// Owned OCCT 7.8/7.9 bridge. No kernel handle crosses this boundary.
#include <Standard_Version.hxx>
#if OCC_VERSION_HEX != 0x070903
#error "cam-kernel currently supports the tested OCCT 7.9.3 release"
#endif
#include <STEPControl_Reader.hxx>
#include <Interface_Static.hxx>
#include <TColStd_SequenceOfAsciiString.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Wire.hxx>
#include <cctype>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_ListIteratorOfListOfShape.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepTools.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <BRep_Tool.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <Poly_Triangulation.hxx>
#include <IMeshData_Status.hxx>
#include <gp_Pln.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Circ.hxx>
#include <gp_Torus.hxx>
#include <cstdlib>
#include <cstring>
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <vector>
#include <algorithm>

namespace {
std::string quote(const std::string& s) {
    std::ostringstream o; o << '"';
    for (unsigned char c : s) {
        if (c == '"' || c == '\\') o << '\\' << c;
        else if (c < 32) o << "\\u" << std::hex << std::setw(4) << std::setfill('0') << int(c) << std::dec;
        else o << c;
    }
    o << '"'; return o.str();
}
std::string xyz(const gp_XYZ& p) { std::ostringstream o; o << std::setprecision(17) << '[' << p.X() << ',' << p.Y() << ',' << p.Z() << ']'; return o.str(); }
std::string bounds(const TopoDS_Shape& s) {
    Bnd_Box b; BRepBndLib::AddOptimal(s,b,false,false);
    double x,y,z,X,Y,Z; b.Get(x,y,z,X,Y,Z);
    return '[' + xyz(gp_XYZ(x,y,z)) + ',' + xyz(gp_XYZ(X,Y,Z)) + ']';
}
struct Array {
    std::vector<std::string> entries;
    void add(std::string s) { entries.push_back(std::move(s)); }
    std::string str() const { std::string s="["; for (size_t i=0;i<entries.size();++i) { if(i) s+=','; s+=entries[i]; } return s+']'; }
};
char* owned(const std::string& s) { char* p=static_cast<char*>(std::malloc(s.size()+1)); if(!p) throw std::bad_alloc(); std::memcpy(p,s.c_str(),s.size()+1); return p; }
std::string surface(const TopoDS_Face& f) {
    BRepAdaptor_Surface a(f);
    std::ostringstream o; o << std::setprecision(17);
    if (a.GetType()==GeomAbs_Plane) {
        gp_Pln p=a.Plane(); gp_Dir n=p.Axis().Direction(); if(f.Orientation()==TopAbs_REVERSED) n.Reverse();
        o << "{\"Plane\":{\"origin\":" << xyz(p.Location().XYZ()) << ",\"normal\":" << xyz(n.XYZ()) << "}}";
    } else if(a.GetType()==GeomAbs_Cylinder) {
        gp_Cylinder c=a.Cylinder();
        gp_Pnt p; gp_Vec du,dv;
        a.D1((a.FirstUParameter()+a.LastUParameter())/2,(a.FirstVParameter()+a.LastVParameter())/2,p,du,dv);
        gp_Vec normal=du.Crossed(dv); if(f.Orientation()==TopAbs_REVERSED) normal.Reverse();
        gp_Vec radial(c.Location(),p); gp_Vec axis(c.Axis().Direction()); radial-=axis*radial.Dot(axis);
        o << "{\"Cylinder\":{\"origin\":" << xyz(c.Location().XYZ()) << ",\"axis\":" << xyz(c.Axis().Direction().XYZ())
          << ",\"radius\":" << c.Radius() << ",\"inward\":" << (normal.Dot(radial)<0?"true":"false") << "}}";
    } else if(a.GetType()==GeomAbs_Torus) {
        gp_Torus t=a.Torus();
        o << "{\"Torus\":{\"origin\":" << xyz(t.Location().XYZ()) << ",\"axis\":" << xyz(t.Axis().Direction().XYZ())
          << ",\"major_radius\":" << t.MajorRadius() << ",\"minor_radius\":" << t.MinorRadius() << "}}";
    } else o << "{\"Unsupported\":{\"description\":\"nonanalytic or unsupported surface\"}}";
    return o.str();
}
std::string curve(const TopoDS_Edge& e) {
    BRepAdaptor_Curve a(e); std::ostringstream o; o << std::setprecision(17);
    if(a.GetType()==GeomAbs_Line) {
        o << "{\"Line\":{\"start\":" << xyz(a.Value(a.FirstParameter()).XYZ()) << ",\"end\":" << xyz(a.Value(a.LastParameter()).XYZ()) << "}}";
    } else if(a.GetType()==GeomAbs_Circle) {
        gp_Circ c=a.Circle();
        o << "{\"CircleArc\":{\"center\":" << xyz(c.Location().XYZ()) << ",\"normal\":" << xyz(c.Axis().Direction().XYZ())
          << ",\"x_axis\":" << xyz(c.Position().XDirection().XYZ()) << ",\"radius\":" << c.Radius()
          << ",\"start_angle\":" << a.FirstParameter() << ",\"end_angle\":" << a.LastParameter() << "}}";
    } else o << "{\"Unsupported\":{\"description\":\"nonanalytic or unsupported curve\"}}";
    return o.str();
}
std::string translate(const char* path,double deflection,double precision) {
    STEPControl_Reader reader;
    // Legacy OCCT parameters are process-global; Rust's service mutex serializes calls.
    Interface_Static::SetCVal("xstep.cascade.unit","MM");
    // Use the STEP uncertainty when provided, with precision as the fallback.
    // Forcing a finer precision and clamping tolerances can turn valid tolerant
    // spline joins into apparent self-intersections (e.g. 0.01 mm source data).
    Interface_Static::SetIVal("read.precision.mode",0);
    Interface_Static::SetRVal("read.precision.val",precision);
    Interface_Static::SetIVal("read.maxprecision.mode",0);
    Interface_Static::SetRVal("read.maxprecision.val",precision);
    if(reader.ReadFile(path)!=IFSelect_RetDone) throw std::runtime_error("OCCT could not read STEP data");
    TColStd_SequenceOfAsciiString length,angle,solidangle; reader.FileUnits(length,angle,solidangle);
    std::string units="Unknown";
    if(length.Length()==1) {
        std::string u=length.Value(1).ToCString(); std::transform(u.begin(),u.end(),u.begin(),[](unsigned char c){return std::tolower(c);});
        if(u=="millimetre"||u=="millimeter"||u=="mm") units="Millimetres";
        else if(u=="inch"||u=="inches"||u=="in") units="Inches";
        else if(u=="metre"||u=="meter"||u=="m") units="Metres";
    }
    if(units=="Unknown") throw std::runtime_error("unknown or mixed STEP source units require an explicit conversion; import rejected");
    reader.SetSystemLengthUnit(1.0);
    const int roots=reader.NbRootsForTransfer();
    const int transferred=reader.TransferRoots();
    if(transferred==0) throw std::runtime_error("STEP contains no translatable roots");
    if(transferred!=roots) throw std::runtime_error("STEP translation was partial; not all root components transferred");
    TopoDS_Shape shape=reader.OneShape();
    Array bodies,faces,edges,loops,vertices,meshvertices,triangles,trianglefaces,placements;
    Array projectionvertices,projectiontriangles,projectionfaces;
    int projectionbase=0;
    constexpr double projection_tolerance=0.0025;
    bool projection_ok=true;
    unsigned long long next=1;
    int meshbase=0,bodycount=0;
    std::vector<std::pair<TopoDS_Shape,unsigned long long>> shared_bodies;
    for(TopExp_Explorer bs(shape,TopAbs_SOLID);bs.More();bs.Next()) {
        TopoDS_Shape placed=bs.Current(); gp_Trsf placement=placed.Location().Transformation();
        TopoDS_Shape b=placed.Located(TopLoc_Location());
        auto shared=std::find_if(shared_bodies.begin(),shared_bodies.end(),[&](const auto& entry){return entry.first.IsSame(b);});
        const bool is_new=shared==shared_bodies.end();
        unsigned long long bodyid=is_new?next++:shared->second;
        std::ostringstream po; po << std::setprecision(17) << "{\"body_id\":" << bodyid << ",\"placement\":{\"rotation\":[";
        for(int row=1;row<=3;++row) { if(row>1) po<<','; po<<'['<<placement.Value(row,1)<<','<<placement.Value(row,2)<<','<<placement.Value(row,3)<<']'; }
        po << "],\"translation\":" << xyz(placement.TranslationPart()) << "}}"; placements.add(po.str());
        if(!is_new) continue;
        shared_bodies.emplace_back(b,bodyid); ++bodycount;
        TopTools_IndexedMapOfShape fm,em,vm; TopExp::MapShapes(b,TopAbs_FACE,fm); TopExp::MapShapes(b,TopAbs_EDGE,em); TopExp::MapShapes(b,TopAbs_VERTEX,vm);
        std::vector<unsigned long long> fi(fm.Extent()+1),ei(em.Extent()+1),vi(vm.Extent()+1);
        for(int i=1;i<=fm.Extent();++i) fi[i]=next++;
        for(int i=1;i<=em.Extent();++i) ei[i]=next++;
        for(int i=1;i<=vm.Extent();++i) vi[i]=next++;
        bodies.add("{\"id\":"+std::to_string(bodyid)+",\"name\":"+quote("Body "+std::to_string(bodycount))+",\"valid_solid\":"+(BRepCheck_Analyzer(b).IsValid()?"true":"false")+",\"bounds\":"+bounds(b)+"}");
        for(int i=1;i<=vm.Extent();++i) vertices.add("{\"id\":"+std::to_string(vi[i])+",\"body_id\":"+std::to_string(bodyid)+",\"point\":"+xyz(BRep_Tool::Pnt(TopoDS::Vertex(vm(i))).XYZ())+"}");
        TopTools_IndexedDataMapOfShapeListOfShape ef; TopExp::MapShapesAndAncestors(b,TopAbs_EDGE,TopAbs_FACE,ef);
        for(int i=1;i<=em.Extent();++i) {
            TopoDS_Edge e=TopoDS::Edge(em(i)); e.Orientation(TopAbs_FORWARD);
            TopoDS_Vertex v1,v2; TopExp::Vertices(e,v1,v2); Array adjacent;
            if(ef.Contains(e)) for(TopTools_ListIteratorOfListOfShape it(ef.FindFromKey(e));it.More();it.Next()) adjacent.add(std::to_string(fi[fm.FindIndex(it.Value())]));
            edges.add("{\"id\":"+std::to_string(ei[i])+",\"body_id\":"+std::to_string(bodyid)+",\"vertices\":["+std::to_string(vi[vm.FindIndex(v1)])+","+std::to_string(vi[vm.FindIndex(v2)])+"],\"face_ids\":"+adjacent.str()+",\"curve\":"+curve(e)+"}");
        }
        BRepTools::Clean(b);
        IMeshTools_Parameters projection_parameters;
        projection_parameters.Deflection=projection_tolerance;
        projection_parameters.DeflectionInterior=projection_tolerance;
        projection_parameters.Angle=0.05;
        projection_parameters.AngleInterior=0.05;
        projection_parameters.Relative=false;
        projection_parameters.EnableControlSurfaceDeflectionAllSurfaces=true;
        BRepMesh_IncrementalMesh projection(b,projection_parameters);
        const int bad_projection=IMeshData_OpenWire|IMeshData_SelfIntersectingWire|IMeshData_Failure|IMeshData_UnorientedWire|IMeshData_TooFewPoints|IMeshData_UserBreak;
        if(!projection.IsDone() || (projection.GetStatusFlags() & bad_projection)) projection_ok=false;
        for(int i=1;i<=fm.Extent();++i) {
            TopLoc_Location loc;
            Handle(Poly_Triangulation) t=BRep_Tool::Triangulation(TopoDS::Face(fm(i)),loc);
            if(t.IsNull() || projectiontriangles.entries.size()+t->NbTriangles()>1000000) projection_ok=false;
            if(!projection_ok) continue;
            for(int n=1;n<=t->NbNodes();++n) projectionvertices.add(xyz(t->Node(n).Transformed(loc.Transformation()).XYZ()));
            for(int n=1;n<=t->NbTriangles();++n) {
                int a,c,d; t->Triangle(n).Get(a,c,d);
                projectiontriangles.add("["+std::to_string(projectionbase+a-1)+","+std::to_string(projectionbase+c-1)+","+std::to_string(projectionbase+d-1)+"]");
                projectionfaces.add(std::to_string(fi[i]));
            }
            projectionbase+=t->NbNodes();
        }
        BRepTools::Clean(b);
        BRepMesh_IncrementalMesh mesh(b,deflection,false,0.35,false);
        if(!mesh.IsDone()) throw std::runtime_error("OCCT tessellation did not complete");
        for(int i=1;i<=fm.Extent();++i) {
            TopoDS_Face f=TopoDS::Face(fm(i)); Array face_loops; TopoDS_Wire outer=BRepTools::OuterWire(f);
            for(TopExp_Explorer ws(f,TopAbs_WIRE);ws.More();ws.Next()) {
                TopoDS_Wire w=TopoDS::Wire(ws.Current()); unsigned long long wid=next++; Array uses;
                for(BRepTools_WireExplorer it(w,f);it.More();it.Next()) {
                    const TopoDS_Edge& e=it.Current(); int idx=em.FindIndex(e);
                    if(!idx) throw std::runtime_error("wire contains unindexed edge");
                    uses.add("{\"edge_id\":"+std::to_string(ei[idx])+",\"reversed\":"+(e.Orientation()==TopAbs_REVERSED?"true":"false")+"}");
                }
                loops.add("{\"id\":"+std::to_string(wid)+",\"body_id\":"+std::to_string(bodyid)+",\"face_id\":"+std::to_string(fi[i])+",\"edge_uses\":"+uses.str()+",\"is_outer\":"+(w.IsSame(outer)?"true":"false")+"}"); face_loops.add(std::to_string(wid));
            }
            faces.add("{\"id\":"+std::to_string(fi[i])+",\"body_id\":"+std::to_string(bodyid)+",\"surface\":"+surface(f)+",\"loop_ids\":"+face_loops.str()+",\"bounds\":"+bounds(f)+"}");
            TopLoc_Location loc; Handle(Poly_Triangulation) t=BRep_Tool::Triangulation(f,loc);
            if(t.IsNull()) throw std::runtime_error("face has no display triangulation");
            for(int n=1;n<=t->NbNodes();++n) meshvertices.add(xyz(t->Node(n).Transformed(loc.Transformation()).XYZ()));
            for(int n=1;n<=t->NbTriangles();++n) {
                int a,c,d; t->Triangle(n).Get(a,c,d); if(f.Orientation()==TopAbs_REVERSED) std::swap(c,d);
                triangles.add("["+std::to_string(meshbase+a-1)+","+std::to_string(meshbase+c-1)+","+std::to_string(meshbase+d-1)+"]"); trianglefaces.add(std::to_string(fi[i]));
            }
            meshbase+=t->NbNodes();
        }
    }
    if(bodycount==0) throw std::runtime_error("STEP contains no solid bodies; surface-only models are not qualified");
    const std::string projection_json=projection_ok ? "{\"chord_tolerance\":0.0025,\"mesh\":{\"vertices\":"+projectionvertices.str()+",\"triangles\":"+projectiontriangles.str()+",\"triangle_faces\":"+projectionfaces.str()+"}}" : "null";
    return "{\"source_units\":"+quote(units)+",\"backend\":"+quote(std::string("OCCT ")+OCC_VERSION_COMPLETE)+",\"warnings\":[\"Assemblies are flattened to placed solid instances; names, colours and external references are not qualified. STEP source tolerances are preserved; import precision is a fallback, not a tolerance clamp. No additional healing is applied.\"],\"placements\":"+placements.str()+",\"topology\":{\"version\":1,\"bodies\":"+bodies.str()+",\"faces\":"+faces.str()+",\"edges\":"+edges.str()+",\"loops\":"+loops.str()+",\"vertices\":"+vertices.str()+",\"projection\":"+projection_json+",\"mesh\":{\"vertices\":"+meshvertices.str()+",\"triangles\":"+triangles.str()+",\"triangle_faces\":"+trianglefaces.str()+"}}}";
}
}
extern "C" char* cam_occt_step(const char* path,double display,double precision,char** error) noexcept {
    *error=nullptr;
    try { return owned(translate(path,display,precision)); }
    catch(const Standard_Failure& e) { try { *error=owned(e.GetMessageString()?e.GetMessageString():"OCCT failure"); } catch(...) {} }
    catch(const std::exception& e) { try { *error=owned(e.what()); } catch(...) {} }
    catch(...) { try { *error=owned("unknown CAD backend failure"); } catch(...) {} }
    return nullptr;
}
extern "C" void cam_occt_free(char* value) noexcept { std::free(value); }
