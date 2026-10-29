// Owned OCCT 7.8/7.9 bridge. No kernel handle crosses this boundary.
#include <Standard_Version.hxx>
#if OCC_VERSION_HEX != 0x070903
#error "cam-kernel currently supports the tested OCCT 7.9.3 release"
#endif
#include <STEPControl_Reader.hxx>
#include <IGESControl_Reader.hxx>
#include <IGESData_IGESModel.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRep_Builder.hxx>
#include <ShapeFix_Solid.hxx>
#include <ShapeFix_Shape.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Solid.hxx>
#include <Interface_Static.hxx>
#include <TColStd_SequenceOfAsciiString.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Wire.hxx>
#include <cctype>
#include <cmath>
#include <limits>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_ListIteratorOfListOfShape.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Curve2d.hxx>
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
#include <gp_Cone.hxx>
#include <gp_Sphere.hxx>
#include <Geom_Ellipse.hxx>
#include <Geom_Parabola.hxx>
#include <Geom_Hyperbola.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_BezierSurface.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_BezierCurve.hxx>
#include <GeomConvert_BSplineCurveToBezierCurve.hxx>
#include <GeomConvert_CurveToAnaCurve.hxx>
#include <GeomAdaptor_Curve.hxx>
#include <GeomConvert.hxx>
#include <Geom_TrimmedCurve.hxx>
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
// Recognize a tensor-product spline extrusion from its complete control net.
// Merely sampling parallel surface normals would miss tapers and bulges.
bool spline_extrusion_axis(const BRepAdaptor_Surface& a,gp_Dir& axis,bool* v_direction=nullptr) {
    if(a.GetType()!=GeomAbs_BSplineSurface) return false;
    const auto b=a.BSpline(); // includes the face location
    if(static_cast<size_t>(b->NbUPoles())*b->NbVPoles()>65536) return false;
    for(bool along_v:{true,false}) {
        const int sections=along_v?b->NbVPoles():b->NbUPoles();
        const int profile=along_v?b->NbUPoles():b->NbVPoles();
        auto pole=[&](int i,int j){return along_v?b->Pole(i,j):b->Pole(j,i);};
        auto weight=[&](int i,int j){return along_v?b->Weight(i,j):b->Weight(j,i);};
        const gp_Vec delta(pole(1,1),pole(1,sections));
        if(delta.Magnitude()<=1e-9) continue;
        const gp_Vec direction=delta.Normalized();
        bool translated=true;
        double previous=-1e-9;
        // Degree elevation adds sections; separable positive weights can change
        // their parameterisation without changing the extrusion's support.
        for(int j=1;j<=sections && translated;++j) {
            const gp_Vec offset(pole(1,1),pole(1,j));
            const double height=offset.Dot(direction),ratio=weight(1,j)/weight(1,1);
            if(!std::isfinite(ratio) || ratio<=0 || height<previous-1e-9
               || (offset-direction*height).Magnitude()>1e-9) { translated=false; break; }
            previous=height;
            for(int i=1;i<=profile;++i) {
                const double w=weight(i,j),base=weight(i,1);
                if(!std::isfinite(w) || !std::isfinite(base) || w<=0 || base<=0
                   || (gp_Vec(pole(i,1),pole(i,j))-offset).Magnitude()>1e-9
                   || std::abs(w/base-ratio)>1e-12*ratio) { translated=false; break; }
            }
        }
        if(translated) {
            axis=gp_Dir(delta);
            if(v_direction) *v_direction=along_v;
            return true;
        }
    }
    return false;
}
std::vector<Handle(Geom_BezierCurve)> spline_spans(const Handle(Geom_Curve)& curve,double first,double last) {
    std::vector<Handle(Geom_BezierCurve)> spans;
    if(const auto spline=Handle(Geom_BSplineCurve)::DownCast(curve); !spline.IsNull()) {
        GeomConvert_BSplineCurveToBezierCurve split(spline,first,last,1e-12);
        if(split.NbArcs()>65536) return spans;
        size_t poles=0;
        for(int i=1;i<=split.NbArcs();++i) {
            auto span=split.Arc(i); poles+=span->NbPoles();
            if(poles>65536) return {};
            spans.push_back(span);
        }
    } else if(const auto bezier=Handle(Geom_BezierCurve)::DownCast(curve); !bezier.IsNull()) {
        auto span=Handle(Geom_BezierCurve)::DownCast(bezier->Copy());
        span->Segment(first,last); spans.push_back(span);
    }
    return spans;
}
long double binomial(int n,int k) {
    long double value=1;
    for(int i=1;i<=k;++i) value=value*(n-i+1)/i;
    return value;
}
// The converter proposes a circle; certify it over every rational Bezier span.
// With positive weights, Bernstein coefficients bound the homogeneous residual
// |Q|^2-r^2 W^2 over the ENTIRE curve, including between fitting samples.
// Checking only poles against a circle is incorrect (arc poles lie outside it).
bool certify_circle(const std::vector<Handle(Geom_BezierCurve)>& spans,const gp_Circ& circle,double tolerance) {
    const double radius=circle.Radius();
    if(spans.empty() || !std::isfinite(radius) || radius<=tolerance) return false;
    for(const auto& span:spans) {
        const int degree=span->Degree();
        double min_weight=std::numeric_limits<double>::infinity(),max_weight=0;
        for(int i=1;i<=span->NbPoles();++i) {
            const double w=span->Weight(i);
            if(!std::isfinite(w) || w<=0) return false;
            min_weight=std::min(min_weight,w); max_weight=std::max(max_weight,w);
            const gp_Vec q(circle.Location(),span->Pole(i));
            if(std::abs(q.Dot(gp_Vec(circle.Axis().Direction())))>tolerance) return false;
        }
        if(min_weight/max_weight<1e-12) return false;
        for(int k=0;k<=2*degree;++k) {
            long double residual=0,denominator=0;
            for(int i=std::max(0,k-degree);i<=std::min(degree,k);++i) {
                const int j=k-i;
                const gp_Vec a(circle.Location(),span->Pole(i+1)),b(circle.Location(),span->Pole(j+1));
                const long double dot=static_cast<long double>(a.X())*b.X()
                    +static_cast<long double>(a.Y())*b.Y()+static_cast<long double>(a.Z())*b.Z();
                const long double factor=binomial(degree,i)*binomial(degree,j)/binomial(2*degree,k)
                    *(span->Weight(i+1)/max_weight)*(span->Weight(j+1)/max_weight);
                residual+=factor*(dot-static_cast<long double>(radius)*radius);
                denominator+=factor;
            }
            const long double limit=tolerance*(2*radius-tolerance)*denominator;
            if(!std::isfinite(residual) || std::abs(residual)>limit) return false;
        }
    }
    return true;
}
// Recognition is deliberately much tighter than display/meshing tolerance and
// independent of source sewing tolerances. Failure leaves the exact spline intact.
constexpr double analytic_tolerance=1e-9;
Handle(Geom_Curve) analytic_spline(const Handle(Geom_Curve)& curve,double first,double last,double& start,double& end) {
    try {
        GeomConvert_CurveToAnaCurve converter(curve);
        Handle(Geom_Curve) result;
        if(!converter.ConvertToAnalytical(analytic_tolerance,result,first,last,start,end)
           || result.IsNull()) return {};
        const auto spans=spline_spans(curve,first,last);
        if(spans.empty()) return {};
        GeomAdaptor_Curve a(result);
        if(a.GetType()==GeomAbs_Circle) {
            if(!certify_circle(spans,a.Circle(),analytic_tolerance)) return {};
            // Preserve traversal as well as the support geometry. A spline can
            // retrace a circle or wind more than once. Require each span's
            // control polygon to advance within one open semicircle, then
            // compare its accumulated sweep with the converter's parameters.
            double sweep=0;
            const auto circle=a.Circle();
            const gp_Vec normal(circle.Axis().Direction());
            const double direction=end>=start?1.:-1.;
            for(const auto& span:spans) {
                const gp_Vec middle(circle.Location(),span->Value(0.5));
                for(int i=1;i<=span->NbPoles();++i) {
                    const gp_Vec p(circle.Location(),span->Pole(i));
                    if(p.Dot(middle)<=0) return {};
                    for(int j=i+1;j<=span->NbPoles();++j) {
                        const gp_Vec q(circle.Location(),span->Pole(j));
                        if(direction*p.Crossed(q).Dot(normal)<-analytic_tolerance*circle.Radius()) return {};
                    }
                }
                const gp_Vec p(circle.Location(),span->StartPoint()),q(circle.Location(),span->EndPoint());
                sweep+=std::atan2(p.Crossed(q).Dot(normal),p.Dot(q));
            }
            if(std::abs(sweep-(end-start))*circle.Radius()>analytic_tolerance) return {};
        } else if(a.GetType()==GeomAbs_Line) {
            const gp_Vec direction=a.Line().Direction();
            double previous=-std::numeric_limits<double>::infinity();
            for(const auto& span:spans) for(int i=1;i<=span->NbPoles();++i) {
                const double parameter=gp_Vec(a.Line().Location(),span->Pole(i)).Dot(direction)*(end>=start?1.:-1.);
                if(!std::isfinite(span->Weight(i)) || span->Weight(i)<=0
                   || a.Line().Distance(span->Pole(i))>analytic_tolerance
                   || parameter<previous-analytic_tolerance) return {};
                previous=parameter;
            }
        } else return {};
        if(!std::isfinite(start) || !std::isfinite(end)
           || result->Value(start).Distance(curve->Value(first))>analytic_tolerance
           || result->Value(end).Distance(curve->Value(last))>analytic_tolerance) return {};
        return result;
    } catch(const Standard_Failure&) {
        return {};
    }
}
bool spline_cylinder(const BRepAdaptor_Surface& a,gp_Cylinder& cylinder) {
    try {
        gp_Dir axis; bool along_v;
        if(!spline_extrusion_axis(a,axis,&along_v)) return false;
        const auto spline=a.BSpline(); // located copy, like spline_extrusion_axis
        const auto section=along_v?spline->VIso((a.FirstVParameter()+a.LastVParameter())/2)
            :spline->UIso((a.FirstUParameter()+a.LastUParameter())/2);
        double start,end;
        const auto converted=analytic_spline(section,section->FirstParameter(),section->LastParameter(),start,end);
        if(converted.IsNull()) return false;
        GeomAdaptor_Curve curve(converted);
        if(curve.GetType()!=GeomAbs_Circle) return false;
        const auto circle=curve.Circle();
        // Use the certified extrusion direction, rather than a circle fitter's
        // slightly rounded normal. Recheck the section against that perpendicular
        // plane so a tilted/elliptic section cannot become a circular cylinder.
        const gp_Circ aligned(gp_Ax2(circle.Location(),axis),circle.Radius());
        if(!certify_circle(spline_spans(section,section->FirstParameter(),section->LastParameter()),aligned,analytic_tolerance)) return false;
        cylinder=gp_Cylinder(gp_Ax3(aligned.Position()),aligned.Radius());
        return true;
    } catch(const Standard_Failure&) {
        return false;
    }
}
// IGES commonly represents flat faces as rational splines. Certify the entire
// positive-weight control net against a plane, not just sampled surface normals
// or boundary points (which would also accept a surface with an interior bulge).
bool spline_plane(const BRepAdaptor_Surface& a,gp_Pln& plane) {
    if(a.GetType()!=GeomAbs_BSplineSurface && a.GetType()!=GeomAbs_BezierSurface) return false;
    gp_Pnt point; gp_Vec du,dv;
    a.D1((a.FirstUParameter()+a.LastUParameter())/2,
         (a.FirstVParameter()+a.LastVParameter())/2,point,du,dv);
    const gp_Vec normal=du.Crossed(dv);
    if(normal.Magnitude()<=1e-12) return false;
    plane=gp_Pln(point,gp_Dir(normal));
    auto coplanar=[&](const auto& spline) {
        for(int u=1;u<=spline->NbUPoles();++u) {
            for(int v=1;v<=spline->NbVPoles();++v) {
                const double weight=spline->Weight(u,v);
                if(!std::isfinite(weight) || weight<=0
                   || plane.Distance(spline->Pole(u,v))>1e-9) return false;
            }
        }
        return true;
    };
    return a.GetType()==GeomAbs_BSplineSurface ? coplanar(a.BSpline()) : coplanar(a.Bezier());
}
std::string surface(const TopoDS_Face& f,const TopTools_IndexedMapOfShape& em,const std::vector<unsigned long long>& edge_ids) {
    BRepAdaptor_Surface a(f);
    std::ostringstream o; o << std::setprecision(17);
    gp_Pln p;
    gp_Cylinder cylinder;
    if (a.GetType()==GeomAbs_Plane || spline_plane(a,p)) {
        if(a.GetType()==GeomAbs_Plane) p=a.Plane();
        gp_Dir n=p.Axis().Direction(); if(f.Orientation()==TopAbs_REVERSED) n.Reverse();
        o << "{\"Plane\":{\"origin\":" << xyz(p.Location().XYZ()) << ",\"normal\":" << xyz(n.XYZ()) << "}}";
    } else if(a.GetType()==GeomAbs_Cylinder || spline_cylinder(a,cylinder)) {
        gp_Cylinder c=a.GetType()==GeomAbs_Cylinder?a.Cylinder():cylinder;
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
    } else if(a.GetType()==GeomAbs_Cone) {
        const auto c=a.Cone();
        o << "{\"Cone\":{\"origin\":" << xyz(c.Location().XYZ())
          << ",\"axis\":" << xyz(c.Axis().Direction().XYZ()) << ",\"radius\":" << c.RefRadius()
          << ",\"semi_angle\":" << c.SemiAngle() << "}}";
    } else if(a.GetType()==GeomAbs_Sphere) {
        const auto s=a.Sphere();
        o << "{\"Sphere\":{\"center\":" << xyz(s.Location().XYZ()) << ",\"radius\":" << s.Radius() << "}}";
    } else {
        gp_Dir axis;
        if(a.GetType()==GeomAbs_SurfaceOfExtrusion || spline_extrusion_axis(a,axis)) {
            if(a.GetType()==GeomAbs_SurfaceOfExtrusion) axis=a.Direction();
            Array sides;
            for(TopExp_Explorer it(f,TopAbs_EDGE);it.More();it.Next()) {
                try {
                    auto edge=TopoDS::Edge(it.Current()); edge.Orientation(TopAbs_FORWARD);
                    BRepAdaptor_Curve curve(edge);
                    BRepAdaptor_Curve2d pcurve(edge,f);
                    const double mid=(curve.FirstParameter()+curve.LastParameter())*0.5;
                    const auto uv=pcurve.Value(mid);
                    gp_Pnt point,on_surface; gp_Vec tangent,du,dv;
                    curve.D1(mid,point,tangent); a.D1(uv.X(),uv.Y(),on_surface,du,dv);
                    gp_Vec normal=du.Crossed(dv);
                    if(f.Orientation()==TopAbs_REVERSED) normal.Reverse();
                    if(tangent.Magnitude()<=1e-12 || normal.Magnitude()<=1e-12
                       || point.Distance(on_surface)>BRep_Tool::Tolerance(edge)+1e-6) continue;
                    tangent.Normalize(); normal.Normalize();
                    if(std::abs(tangent.Dot(normal))>1e-6) continue;
                    sides.add("{\"edge_id\":"+std::to_string(edge_ids[em.FindIndex(edge)])
                        +",\"tangent\":"+xyz(tangent.XYZ())+",\"outward\":"+xyz(normal.XYZ())+"}");
                } catch(const Standard_Failure&) {
                    // Keep valid solids inspectable when a degenerate seam
                    // cannot supply a local machining orientation.
                }
            }
            o << "{\"LinearExtrusion\":{\"axis\":" << xyz(axis.XYZ()) << ",\"edge_sides\":" << sides.str() << "}}";
        } else {
            const std::string kind=a.GetType()==GeomAbs_BSplineSurface?"B-spline surface"
                :a.GetType()==GeomAbs_BezierSurface?"Bezier surface"
                :a.GetType()==GeomAbs_SurfaceOfRevolution?"surface of revolution"
                :a.GetType()==GeomAbs_OffsetSurface?"offset surface":"surface";
            o << "{\"Unsupported\":{\"description\":" << quote(kind+" retained in the CAD mesh; analytic feature extraction is unavailable") << "}}";
        }
    }
    return o.str();
}
std::string bezier_span(const Handle(Geom_BezierCurve)& curve) {
    Array poles,weights;
    double min_weight=std::numeric_limits<double>::infinity(),max_weight=0;
    for(int i=1;i<=curve->NbPoles();++i) {
        const double weight=curve->Weight(i);
        if(!std::isfinite(weight) || weight<=0) throw std::runtime_error("invalid rational spline weights");
        min_weight=std::min(min_weight,weight); max_weight=std::max(max_weight,weight);
        poles.add(xyz(curve->Pole(i).XYZ()));
        std::ostringstream w; w << std::setprecision(17) << curve->Weight(i); weights.add(w.str());
    }
    if(min_weight/max_weight<1e-12) throw std::runtime_error("rational spline weight range exceeds machining precision budget");
    return "{\"poles\":"+poles.str()+",\"weights\":"+weights.str()+"}";
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
    } else if(a.GetType()==GeomAbs_BSplineCurve || a.GetType()==GeomAbs_BezierCurve
              || a.GetType()==GeomAbs_Ellipse || a.GetType()==GeomAbs_Parabola || a.GetType()==GeomAbs_Hyperbola) {
        try {
            Handle(Geom_Curve) spline;
            double spline_first=a.FirstParameter(),spline_last=a.LastParameter();
            if(a.GetType()==GeomAbs_BSplineCurve) spline=a.BSpline();
            else if(a.GetType()==GeomAbs_BezierCurve) spline=a.Bezier();
            else {
                Handle(Geom_Curve) conic;
                if(a.GetType()==GeomAbs_Ellipse) conic=new Geom_Ellipse(a.Ellipse());
                else if(a.GetType()==GeomAbs_Parabola) conic=new Geom_Parabola(a.Parabola());
                else conic=new Geom_Hyperbola(a.Hyperbola());
                // Exact rational conversion of the finite trim; preserve location
                // and direction, with the converted curve's own parameter range.
                spline=GeomConvert::CurveToBSplineCurve(new Geom_TrimmedCurve(conic,spline_first,spline_last));
                spline_first=spline->FirstParameter(); spline_last=spline->LastParameter();
            }
            double first,last;
            const auto analytic=analytic_spline(spline,spline_first,spline_last,first,last);
            if(!analytic.IsNull()) {
                GeomAdaptor_Curve converted(analytic);
                if(converted.GetType()==GeomAbs_Line) {
                    o << "{\"Line\":{\"start\":" << xyz(a.Value(a.FirstParameter()).XYZ())
                      << ",\"end\":" << xyz(a.Value(a.LastParameter()).XYZ()) << "}}";
                } else {
                    const auto c=converted.Circle();
                    o << "{\"CircleArc\":{\"center\":" << xyz(c.Location().XYZ())
                      << ",\"normal\":" << xyz(c.Axis().Direction().XYZ())
                      << ",\"x_axis\":" << xyz(c.Position().XDirection().XYZ()) << ",\"radius\":" << c.Radius()
                      << ",\"start_angle\":" << first << ",\"end_angle\":" << last << "}}";
                }
                return o.str();
            }
            Array spans;
            if(const auto bspline=Handle(Geom_BSplineCurve)::DownCast(spline); !bspline.IsNull()) {
                GeomConvert_BSplineCurveToBezierCurve split(bspline,spline_first,spline_last,1e-12);
                if(split.NbArcs()>65536) throw std::runtime_error("spline span budget exceeded");
                size_t pole_count=0;
                for(int i=1;i<=split.NbArcs();++i) {
                    const auto span=split.Arc(i); pole_count+=span->NbPoles();
                    if(pole_count>65536) throw std::runtime_error("spline control-point budget exceeded");
                    spans.add(bezier_span(span));
                }
            } else {
                auto curve=Handle(Geom_BezierCurve)::DownCast(spline); // owned, located copy
                curve->Segment(spline_first,spline_last);
                spans.add(bezier_span(curve));
            }
            o << "{\"BezierSpans\":{\"spans\":" << spans.str() << "}}";
        } catch(const Standard_Failure&) {
            o << "{\"Unsupported\":{\"description\":\"spline could not be converted to exact trimmed Bezier spans\"}}";
        } catch(const std::runtime_error& error) {
            o << "{\"Unsupported\":{\"description\":" << quote(error.what()) << "}}";
        }
    } else o << "{\"Unsupported\":{\"description\":\"nonanalytic or unsupported curve\"}}";
    return o.str();
}
void configure_precision(double precision) {
    // Legacy OCCT parameters are process-global; Rust's service mutex serializes calls.
    Interface_Static::SetCVal("xstep.cascade.unit","MM");
    // Use the STEP uncertainty when provided, with precision as the fallback.
    // Forcing a finer precision and clamping tolerances can turn valid tolerant
    // spline joins into apparent self-intersections (e.g. 0.01 mm source data).
    Interface_Static::SetIVal("read.precision.mode",0);
    Interface_Static::SetRVal("read.precision.val",precision);
    Interface_Static::SetIVal("read.maxprecision.mode",0);
    Interface_Static::SetRVal("read.maxprecision.val",precision);
}
TopoDS_Shape solidify_closed_surfaces(const TopoDS_Shape& shape,double precision,const std::string& format) {
    // Preserve explicit B-rep solids. Many IGES writers instead export separate
    // trimmed surfaces: sew only those free faces and require closed valid shells.
    BRep_Builder builder; TopoDS_Compound result; builder.MakeCompound(result);
    for(TopExp_Explorer it(shape,TopAbs_SOLID);it.More();it.Next()) builder.Add(result,it.Current());
    BRepBuilderAPI_Sewing sewing(precision);
    // Preserve the transfer engine's local edge tolerances when joining faces.
    // Never derive sewing tolerance from display tessellation or model size.
    sewing.SetLocalTolerancesMode(true);
    int free_faces=0;
    for(TopExp_Explorer it(shape,TopAbs_FACE,TopAbs_SOLID);it.More();it.Next()) {
        sewing.Add(it.Current()); ++free_faces;
    }
    if(free_faces) {
        sewing.Perform();
        const auto sewn=sewing.SewedShape();
        if(sewn.IsNull()) throw std::runtime_error(format+" surface sewing failed");
        for(TopExp_Explorer it(sewn,TopAbs_FACE,TopAbs_SHELL);it.More();it.Next())
            throw std::runtime_error(format+" contains open surfaces; a closed solid is required");
        std::vector<TopoDS_Solid> surface_solids;
        for(TopExp_Explorer it(sewn,TopAbs_SHELL);it.More();it.Next()) {
            const auto shell=TopoDS::Shell(it.Current());
            if(!BRep_Tool::IsClosed(shell)) throw std::runtime_error(format+" contains open surfaces; a closed solid is required");
            ShapeFix_Solid fixer;
            const auto solid=fixer.SolidFromShell(shell);
            if(solid.IsNull() || !BRepCheck_Analyzer(solid).IsValid()) throw std::runtime_error(format+" surface shell could not form a valid solid");
            // A surface-only export does not identify outer versus void shells.
            // Reject nesting rather than fill a cavity with another solid body.
            for(const auto& other:surface_solids) {
                auto inside=[&](const TopoDS_Solid& inner,const TopoDS_Solid& outer) {
                    TopExp_Explorer vertex(inner,TopAbs_VERTEX);
                    if(!vertex.More()) throw std::runtime_error(format+" shell has no vertices");
                    BRepClass3d_SolidClassifier classifier(outer,BRep_Tool::Pnt(TopoDS::Vertex(vertex.Current())),precision);
                    return classifier.State()==TopAbs_IN;
                };
                if(inside(solid,other) || inside(other,solid))
                    throw std::runtime_error(format+" contains nested surface shells; export B-rep solids to preserve voids");
            }
            surface_solids.push_back(solid);
            builder.Add(result,solid);
        }
    }
    for(TopExp_Explorer it(shape,TopAbs_EDGE,TopAbs_FACE);it.More();it.Next())
        throw std::runtime_error(format+" contains wire geometry; only solids and closed surface shells are supported");
    return result;
}
TopoDS_Shape read_step(const char* path,double precision,std::string& units) {
    STEPControl_Reader reader;
    configure_precision(precision);
    if(reader.ReadFile(path)!=IFSelect_RetDone) throw std::runtime_error("OCCT could not read STEP data");
    TColStd_SequenceOfAsciiString length,angle,solidangle; reader.FileUnits(length,angle,solidangle);
    units="Unknown";
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
    return solidify_closed_surfaces(reader.OneShape(),precision,"STEP");
}
TopoDS_Shape read_iges(const char* path,double precision,std::string& units) {
    IGESControl_Reader reader;
    configure_precision(precision);
    // Include hidden roots too, so components are never silently omitted.
    reader.SetReadVisible(false);
    if(reader.ReadFile(path)!=IFSelect_RetDone) throw std::runtime_error("OCCT could not read IGES data");
    const auto model=reader.IGESModel();
    if(model.IsNull()) throw std::runtime_error("IGES has no model");
    const auto& header=model->GlobalSection();
    switch(header.UnitFlag()) {
        case 1: units="Inches"; break;
        case 2: units="Millimetres"; break;
        case 6: units="Metres"; break;
        default: throw std::runtime_error("unsupported or unknown IGES source units; supported units are millimetres, inches and metres");
    }
    // IGES uses the model's cascade unit for transfer; 1.0 is millimetres.
    model->ChangeGlobalSection().SetCascadeUnit(1.0);
    const int roots=reader.NbRootsForTransfer();
    const int transferred=reader.TransferRoots();
    if(transferred==0) throw std::runtime_error("IGES contains no translatable roots");
    if(transferred!=roots) throw std::runtime_error("IGES translation was partial; not all root components transferred");
    const TopoDS_Shape shape=reader.OneShape();

    return solidify_closed_surfaces(shape,precision,"IGES");
}
// Best-effort repair of an invalid transferred solid. Work on an owned copy,
// respect existing source tolerances, and retain the original if repair removes
// topology, moves bounds beyond that budget, or still fails validation.
TopoDS_Shape repaired_solid(const TopoDS_Shape& original,double precision) {
    try {
        double tolerance=precision;
        for(TopExp_Explorer it(original,TopAbs_FACE);it.More();it.Next())
            tolerance=std::max(tolerance,BRep_Tool::Tolerance(TopoDS::Face(it.Current())));
        for(TopExp_Explorer it(original,TopAbs_EDGE);it.More();it.Next())
            tolerance=std::max(tolerance,BRep_Tool::Tolerance(TopoDS::Edge(it.Current())));
        for(TopExp_Explorer it(original,TopAbs_VERTEX);it.More();it.Next())
            tolerance=std::max(tolerance,BRep_Tool::Tolerance(TopoDS::Vertex(it.Current())));
        if(!std::isfinite(tolerance)) return original;
        BRepBuilderAPI_Copy copy(original,true,false);
        ShapeFix_Shape fixer(copy.Shape());
        fixer.SetPrecision(precision);
        fixer.SetMaxTolerance(tolerance);
        fixer.Perform();
        const auto result=fixer.Shape();
        if(result.IsNull() || result.ShapeType()!=TopAbs_SOLID || !BRepCheck_Analyzer(result).IsValid()) return original;
        for(auto type:{TopAbs_FACE,TopAbs_WIRE,TopAbs_EDGE,TopAbs_VERTEX}) {
            TopTools_IndexedMapOfShape before,after;
            TopExp::MapShapes(original,type,before); TopExp::MapShapes(result,type,after);
            if(before.Extent()!=after.Extent()) return original;
        }
        Bnd_Box before,after;
        BRepBndLib::AddOptimal(original,before,false,false); BRepBndLib::AddOptimal(result,after,false,false);
        double a[6],b[6];
        before.Get(a[0],a[1],a[2],a[3],a[4],a[5]); after.Get(b[0],b[1],b[2],b[3],b[4],b[5]);
        for(int i=0;i<6;++i) if(!std::isfinite(b[i]) || std::abs(a[i]-b[i])>tolerance) return original;
        return result;
    } catch(const Standard_Failure&) {
        return original;
    }
}
std::string translate(const char* path,bool iges,double deflection,double precision) {
    std::string units;
    const TopoDS_Shape shape=iges?read_iges(path,precision,units):read_step(path,precision,units);
    Array bodies,faces,edges,loops,vertices,meshvertices,triangles,trianglefaces,placements,warnings;
    size_t unsupported_surfaces=0,unsupported_curves=0;
    Array projectionvertices,projectiontriangles,projectionfaces;
    int projectionbase=0;
    constexpr double projection_tolerance=0.0025;
    bool projection_ok=true;
    std::string projection_issue;
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
        bool valid_solid=BRepCheck_Analyzer(b).IsValid();
        if(!valid_solid) {
            b=repaired_solid(b,precision);
            valid_solid=BRepCheck_Analyzer(b).IsValid();
            warnings.add(quote("Body "+std::to_string(bodycount)+(valid_solid
                ?": repaired invalid CAD topology within source/import tolerances; topology counts and bounds preserved."
                :": CAD solid validation failed and bounded repair did not succeed; inspect or repair the source before feature machining.")));
        }
        TopTools_IndexedMapOfShape fm,em,vm; TopExp::MapShapes(b,TopAbs_FACE,fm); TopExp::MapShapes(b,TopAbs_EDGE,em); TopExp::MapShapes(b,TopAbs_VERTEX,vm);
        std::vector<unsigned long long> fi(fm.Extent()+1),ei(em.Extent()+1),vi(vm.Extent()+1);
        for(int i=1;i<=fm.Extent();++i) fi[i]=next++;
        for(int i=1;i<=em.Extent();++i) ei[i]=next++;
        for(int i=1;i<=vm.Extent();++i) vi[i]=next++;
        bodies.add("{\"id\":"+std::to_string(bodyid)+",\"name\":"+quote("Body "+std::to_string(bodycount))+",\"valid_solid\":"+(valid_solid?"true":"false")+",\"bounds\":"+bounds(b)+"}");
        for(int i=1;i<=vm.Extent();++i) vertices.add("{\"id\":"+std::to_string(vi[i])+",\"body_id\":"+std::to_string(bodyid)+",\"point\":"+xyz(BRep_Tool::Pnt(TopoDS::Vertex(vm(i))).XYZ())+"}");
        TopTools_IndexedDataMapOfShapeListOfShape ef; TopExp::MapShapesAndAncestors(b,TopAbs_EDGE,TopAbs_FACE,ef);
        for(int i=1;i<=em.Extent();++i) {
            TopoDS_Edge e=TopoDS::Edge(em(i)); e.Orientation(TopAbs_FORWARD);
            TopoDS_Vertex v1,v2; TopExp::Vertices(e,v1,v2); Array adjacent;
            if(ef.Contains(e)) for(TopTools_ListIteratorOfListOfShape it(ef.FindFromKey(e));it.More();it.Next()) adjacent.add(std::to_string(fi[fm.FindIndex(it.Value())]));
            const auto edge_curve=curve(e);
            if(edge_curve.rfind("{\"Unsupported\":",0)==0) ++unsupported_curves;
            edges.add("{\"id\":"+std::to_string(ei[i])+",\"body_id\":"+std::to_string(bodyid)+",\"vertices\":["+std::to_string(vi[vm.FindIndex(v1)])+","+std::to_string(vi[vm.FindIndex(v2)])+"],\"face_ids\":"+adjacent.str()+",\"curve\":"+edge_curve+"}");
        }
        BRepTools::Clean(b);
        IMeshTools_Parameters projection_parameters;
        projection_parameters.Deflection=projection_tolerance;
        projection_parameters.DeflectionInterior=projection_tolerance;
        // Linear deflection governs machining accuracy. An unnecessarily tight
        // angular constraint overmeshes small rounds independently of that bound.
        projection_parameters.Angle=0.35;
        projection_parameters.AngleInterior=0.35;
        projection_parameters.Relative=false;
        projection_parameters.EnableControlSurfaceDeflectionAllSurfaces=true;
        BRepMesh_IncrementalMesh projection(b,projection_parameters);
        const int bad_projection=IMeshData_OpenWire|IMeshData_SelfIntersectingWire|IMeshData_Failure|IMeshData_UnorientedWire|IMeshData_TooFewPoints|IMeshData_UserBreak;
        if(!projection.IsDone() || (projection.GetStatusFlags() & bad_projection)) {
            projection_ok=false;
            if(projection_issue.empty()) projection_issue="Body "+std::to_string(bodycount)+": XY projection meshing failed (OCCT status "+std::to_string(projection.GetStatusFlags())+").";
        }
        for(int i=1;i<=fm.Extent();++i) {
            TopLoc_Location loc;
            Handle(Poly_Triangulation) t=BRep_Tool::Triangulation(TopoDS::Face(fm(i)),loc);
            if(t.IsNull() || projectiontriangles.entries.size()+t->NbTriangles()>1000000) {
                projection_ok=false;
                if(projection_issue.empty()) projection_issue="Body "+std::to_string(bodycount)+": "+(t.IsNull()?"a face has no XY projection triangulation.":"XY projection exceeds the 1,000,000 triangle budget.");
            }
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
            const auto face_surface=surface(f,em,ei);
            if(face_surface.rfind("{\"Unsupported\":",0)==0) ++unsupported_surfaces;
            faces.add("{\"id\":"+std::to_string(fi[i])+",\"body_id\":"+std::to_string(bodyid)+",\"surface\":"+face_surface+",\"loop_ids\":"+face_loops.str()+",\"bounds\":"+bounds(f)+"}");
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
    if(bodycount==0) throw std::runtime_error(std::string(iges?"IGES":"STEP")+" contains no solid bodies; open surface-only models are not supported");
    const std::string projection_json=projection_ok ? "{\"chord_tolerance\":0.0025,\"mesh\":{\"vertices\":"+projectionvertices.str()+",\"triangles\":"+projectiontriangles.str()+",\"triangle_faces\":"+projectionfaces.str()+"}}" : "null";
    if(!projection_issue.empty()) warnings.add(quote(projection_issue));
    if(unsupported_surfaces) warnings.add(quote(std::to_string(unsupported_surfaces)+" CAD faces have no analytic feature representation; their CAD mesh is retained."));
    if(unsupported_curves) warnings.add(quote(std::to_string(unsupported_curves)+" CAD edges have no supported exact curve representation; selections using those edges need different geometry."));
    return "{\"source_units\":"+quote(units)+",\"backend\":"+quote(std::string("OCCT ")+OCC_VERSION_COMPLETE)+",\"warnings\":"+warnings.str()+",\"placements\":"+placements.str()+",\"topology\":{\"version\":1,\"bodies\":"+bodies.str()+",\"faces\":"+faces.str()+",\"edges\":"+edges.str()+",\"loops\":"+loops.str()+",\"vertices\":"+vertices.str()+",\"projection\":"+projection_json+",\"mesh\":{\"vertices\":"+meshvertices.str()+",\"triangles\":"+triangles.str()+",\"triangle_faces\":"+trianglefaces.str()+"}}}";
}
}
extern "C" char* cam_occt_read(const char* path,int iges,double display,double precision,char** error) noexcept {
    *error=nullptr;
    try { return owned(translate(path,iges!=0,display,precision)); }
    catch(const Standard_Failure& e) { try { *error=owned(e.GetMessageString()?e.GetMessageString():"OCCT failure"); } catch(...) {} }
    catch(const std::exception& e) { try { *error=owned(e.what()); } catch(...) {} }
    catch(...) { try { *error=owned("unknown CAD backend failure"); } catch(...) {} }
    return nullptr;
}
extern "C" void cam_occt_free(char* value) noexcept { std::free(value); }
