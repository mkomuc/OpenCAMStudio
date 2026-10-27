// Original redistribution-safe IGES fixtures. Compile against OCCT 7.9.3.
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRep_Tool.hxx>
#include <BRepTools.hxx>
#include <GeomConvert.hxx>
#include <Geom_RectangularTrimmedSurface.hxx>
#include <Geom_BSplineSurface.hxx>
#include <TopoDS.hxx>
#include <BRep_Builder.hxx>
#include <IGESControl_Controller.hxx>
#include <IGESControl_Writer.hxx>
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
}
