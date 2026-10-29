// Original redistribution-safe IGES fixtures. Compile against OCCT 7.9.3.
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_NurbsConvert.hxx>
#include <BRepBuilderAPI_GTransform.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <Geom_Ellipse.hxx>
#include <Geom_Parabola.hxx>
#include <Geom_Hyperbola.hxx>
#include <BRep_Tool.hxx>
#include <BRepTools.hxx>
#include <GeomConvert.hxx>
#include <Geom_RectangularTrimmedSurface.hxx>
#include <Geom_BSplineSurface.hxx>
#include <TopoDS.hxx>
#include <BRep_Builder.hxx>
#include <IGESControl_Controller.hxx>
#include <IGESControl_Writer.hxx>
#include <STEPControl_Writer.hxx>
#include <TopoDS_Compound.hxx>
#include <TopExp_Explorer.hxx>
#include <gp_Ax2.hxx>
#include <gp_Trsf.hxx>
#include <stdexcept>
#include <string>

TopoDS_Shape spline_box(const TopoDS_Shape& box,bool bulge) {
    TopoDS_Compound faces; BRep_Builder builder; builder.MakeCompound(faces);
    for(TopExp_Explorer it(box,TopAbs_FACE);it.More();it.Next()) {
        auto face=TopoDS::Face(it.Current());
        double u0,u1,v0,v1; BRepTools::UVBounds(face,u0,u1,v0,v1);
        auto trimmed=new Geom_RectangularTrimmedSurface(BRep_Tool::Surface(face),u0,u1,v0,v1);
        auto spline=GeomConvert::SurfaceToBSplineSurface(trimmed);
        spline->IncreaseDegree(2,2);
        // The boundary stays exactly flat while one face has a raised interior.
        // Recognizing planes from their edges or a few samples is insufficient.
        if(bulge && std::abs(spline->Pole(1,1).Z())<1e-9
                 && std::abs(spline->Pole(3,3).Z())<1e-9) {
            auto pole=spline->Pole(2,2); pole.SetZ(2.0); spline->SetPole(2,2,pole);
        }
        auto converted=BRepBuilderAPI_MakeFace(spline,1e-7).Face();
        converted.Orientation(face.Orientation());
        builder.Add(faces,converted);
    }
    return faces;
}

TopoDS_Shape conic_prism(const Handle(Geom_Curve)& curve,double first,double last) {
    BRepBuilderAPI_MakeWire wire;
    wire.Add(BRepBuilderAPI_MakeEdge(curve,first,last).Edge());
    if(curve->Value(first).Distance(curve->Value(last))>1e-7)
        wire.Add(BRepBuilderAPI_MakeEdge(curve->Value(last),curve->Value(first)).Edge());
    return BRepPrimAPI_MakePrism(BRepBuilderAPI_MakeFace(wire.Wire()).Face(),gp_Vec(0,0,-10)).Shape();
}

int main(int argc,char** argv) {
    if(argc!=2) return 2;
    IGESControl_Controller::Init();
    const std::string out=argv[1];
    const auto box=BRepPrimAPI_MakeBox(gp_Pnt(0,0,-10),60,40,10).Shape();
    auto write=[&](const TopoDS_Shape& shape,const char* name,const char* unit,int mode) {
        IGESControl_Writer writer(unit,mode);
        if(!writer.AddShape(shape) || !writer.Write((out+"/"+name).c_str()))
            throw std::runtime_error("IGES fixture write failed");
    };
    auto write_step=[&](const TopoDS_Shape& shape,const char* name) {
        STEPControl_Writer writer;
        if(writer.Transfer(shape,STEPControl_AsIs)!=IFSelect_RetDone
           || writer.Write((out+"/"+name).c_str())!=IFSelect_RetDone)
            throw std::runtime_error("STEP fixture write failed");
    };
    write(box,"box-mm.iges","MM",1);
    write(box,"unsupported-unit-cm.iges","CM",1);
    write(box,"box-inch.igs","INCH",1);
    write(box,"box-m.iges","M",1);
    write(box,"box-surfaces-mm.igs","MM",0);
    write(spline_box(box,false),"spline-box-surfaces-mm.igs","MM",0);
    write(spline_box(box,true),"spline-bulge-surfaces-mm.igs","MM",0);
    TopExp_Explorer face(box,TopAbs_FACE);
    write(face.Current(),"open-surface-mm.iges","MM",0);
    TopExp_Explorer edge(box,TopAbs_EDGE);
    write(edge.Current(),"wire-mm.iges","MM",0);
    TopoDS_Compound repeated; BRep_Builder builder; builder.MakeCompound(repeated);
    builder.Add(repeated,box);
    gp_Trsf tr; tr.SetTranslation(gp_Vec(80,0,0));
    const auto moved=BRepBuilderAPI_Transform(box,tr,true).Shape();
    builder.Add(repeated,moved);
    write(repeated,"two-boxes-mm.iges","MM",1);
    write(repeated,"two-boxes-surfaces-mm.iges","MM",0);
    TopoDS_Compound nested; builder.MakeCompound(nested); builder.Add(nested,box);
    builder.Add(nested,BRepPrimAPI_MakeBox(gp_Pnt(10,10,-8),10,10,4).Shape());
    write(nested,"nested-surfaces-mm.iges","MM",0);
    builder.Add(repeated,face.Current());
    write(repeated,"solids-and-open-surface-mm.iges","MM",1);
    const auto bore=BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(30,20,-10),gp_Dir(0,0,1)),2.5,10).Shape();
    write(BRepAlgoAPI_Cut(box,bore).Shape(),"box-hole-mm.iges","MM",1);
    const auto spline_hole=BRepBuilderAPI_NurbsConvert(BRepAlgoAPI_Cut(box,bore).Shape(),true).Shape();
    write(spline_hole,"spline-hole-mm.iges","MM",1);
    STEPControl_Writer step;
    if(step.Transfer(spline_hole,STEPControl_AsIs)!=IFSelect_RetDone
       || step.Write((out+"/spline-hole-mm.step").c_str())!=IFSelect_RetDone)
        throw std::runtime_error("spline STEP fixture write failed");
    gp_GTrsf stretch; stretch.SetValue(1,1,1.001);
    write(BRepBuilderAPI_GTransform(spline_hole,stretch,true).Shape(),"spline-elliptic-hole-mm.iges","MM",1);
    // Also cover located curves/surfaces and a feature not parallel to world Z.
    gp_Trsf tilt; tilt.SetRotation(gp_Ax1(gp_Pnt(0,0,0),gp_Dir(1,0,0)),0.6);
    tilt.SetTranslationPart(gp_Vec(100,-50,40));
    write(BRepBuilderAPI_Transform(spline_hole,tilt,true).Shape(),"spline-hole-rotated-mm.iges","MM",1);
    // Same circular rims, but a barrel-shaped wall between them. A recognizer
    // that fits only the boundary would incorrectly turn this into a cylinder.
    const auto barrel=BRepBuilderAPI_NurbsConvert(bore,true).Shape();
    for(TopExp_Explorer it(barrel,TopAbs_FACE);it.More();it.Next()) {
        auto face=TopoDS::Face(it.Current());
        auto spline=Handle(Geom_BSplineSurface)::DownCast(BRep_Tool::Surface(face));
        if(spline.IsNull()) continue;
        spline->IncreaseDegree(2,2);
        for(int u=1;u<=spline->NbUPoles();++u) {
            auto p=spline->Pole(u,2);
            p.SetX(30+1.1*(p.X()-30)); p.SetY(20+1.1*(p.Y()-20));
            spline->SetPole(u,2,p);
        }
    }
    write(barrel,"spline-barrel-mm.iges","MM",1);

    auto elevated=BRepBuilderAPI_NurbsConvert(BRepAlgoAPI_Cut(box,bore).Shape(),true).Shape();
    for(TopExp_Explorer it(elevated,TopAbs_FACE);it.More();it.Next()) {
        auto surface=Handle(Geom_BSplineSurface)::DownCast(BRep_Tool::Surface(TopoDS::Face(it.Current())));
        if(!surface.IsNull()) surface->IncreaseDegree(3,3);
    }
    write(elevated,"elevated-spline-hole-mm.iges","MM",1);
    write_step(elevated,"elevated-spline-hole-mm.step");

    const gp_Ax2 plane(gp_Pnt(30,20,0),gp_Dir(0,0,1));
    const auto ellipse=conic_prism(new Geom_Ellipse(plane,10,5),0,2*std::acos(-1.));
    const auto parabola=conic_prism(new Geom_Parabola(plane,3),-2,2);
    const auto hyperbola=conic_prism(new Geom_Hyperbola(plane,5,3),-1,1);
    write(ellipse,"ellipse-prism-mm.iges","MM",1); write_step(ellipse,"ellipse-prism-mm.step");
    // OCCT 7.9.3's IGES writer omits the native parabolic extrusion's side.
    // Keep that incomplete export to test that repair cannot certify a shell
    // as a solid, then export the complete NURBS equivalent for conic coverage.
    write(parabola,"incomplete-parabola-mm.iges","MM",1);
    write(BRepBuilderAPI_NurbsConvert(parabola,true).Shape(),"parabola-prism-mm.iges","MM",1);
    write_step(parabola,"parabola-prism-mm.step");
    write_step(hyperbola,"hyperbola-prism-mm.step");
    write(BRepBuilderAPI_NurbsConvert(hyperbola,true).Shape(),"hyperbola-prism-mm.iges","MM",1);

    TopoDS_Compound round; builder.MakeCompound(round);
    builder.Add(round,BRepPrimAPI_MakeCone(5,2,10).Shape());
    builder.Add(round,BRepPrimAPI_MakeSphere(gp_Pnt(30,0,0),5).Shape());
    write_step(round,"cone-sphere-mm.step"); write(round,"cone-sphere-mm.iges","MM",1);

    TopExp_Explorer shell(box,TopAbs_SHELL);
    write_step(shell.Current(),"closed-shell-mm.step");
    TopoDS_Compound incomplete; builder.MakeCompound(incomplete);
    builder.Add(incomplete,box); builder.Add(incomplete,face.Current());
    write_step(incomplete,"solid-and-open-shell-mm.step");
    TopoDS_Compound nested_shells; builder.MakeCompound(nested_shells);
    builder.Add(nested_shells,shell.Current());
    const auto inner=BRepPrimAPI_MakeBox(gp_Pnt(10,10,-8),10,10,4).Shape();
    TopExp_Explorer inner_shell(inner,TopAbs_SHELL); builder.Add(nested_shells,inner_shell.Current());
    write_step(nested_shells,"nested-shells-mm.step");
}
