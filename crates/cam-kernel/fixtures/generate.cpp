// Original redistribution-safe qualification fixtures, authored for this project.
// Compile manually against OCCT 7.9; generated STEP files are checked in.
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRep_Builder.hxx>
#include <TopoDS_Compound.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <STEPControl_Writer.hxx>
#include <Interface_Static.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Pnt.hxx>
#include <gp_Dir.hxx>
#include <gp_Trsf.hxx>
#include <stdexcept>
#include <string>
int main(int argc,char**argv) {
 if(argc!=2) return 2;
 std::string out=argv[1];
 auto box=BRepPrimAPI_MakeBox(gp_Pnt(0,0,-10),60,40,10).Shape();
 auto cavity=BRepPrimAPI_MakeBox(gp_Pnt(10,8,-6),40,24,6).Shape();
 auto island=BRepPrimAPI_MakeBox(gp_Pnt(24,16,-6),12,8,6).Shape();
 auto plate=BRepAlgoAPI_Cut(box,BRepAlgoAPI_Cut(cavity,island).Shape()).Shape();
 for(auto p:{gp_Pnt(5,5,-10),gp_Pnt(55,5,-10),gp_Pnt(55,35,-10),gp_Pnt(5,35,-10)}) {
   plate=BRepAlgoAPI_Cut(plate,BRepPrimAPI_MakeCylinder(gp_Ax2(p,gp_Dir(0,0,1)),2.5,10).Shape()).Shape();
 }
 auto write=[&](const TopoDS_Shape&s,const char*name,const char*unit){
   STEPControl_Writer w; Interface_Static::SetCVal("write.step.unit",unit);
   if(w.Transfer(s,STEPControl_AsIs)!=IFSelect_RetDone || w.Write((out+"/"+name).c_str())!=IFSelect_RetDone) throw std::runtime_error("fixture write failed");
 };
 TopoDS_Compound repeated;BRep_Builder builder;builder.MakeCompound(repeated);builder.Add(repeated,box);
 gp_Trsf copy;copy.SetTranslation(gp_Vec(80,0,0));builder.Add(repeated,box.Moved(TopLoc_Location(copy)));
 write(repeated,"repeated-boxes.step","MM");
 write(box,"box-mm.step","MM"); write(plate,"pocket-island-holes-mm.step","MM");
 write(plate,"pocket-island-holes-inch.step","INCH");
 gp_Trsf tr;tr.SetRotation(gp_Ax1(gp_Pnt(0,0,0),gp_Dir(1,0,0)),0.6);tr.SetTranslationPart(gp_Vec(100,-50,40));
 write(BRepBuilderAPI_Transform(plate,tr,true).Shape(),"pocket-island-holes-rotated.step","MM");
}
