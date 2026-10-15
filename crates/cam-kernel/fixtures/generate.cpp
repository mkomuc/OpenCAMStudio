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
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <Geom_BSplineCurve.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <TColStd_Array1OfInteger.hxx>
#include <TColStd_Array1OfReal.hxx>
#include <gp_Pln.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Pnt.hxx>
#include <gp_Dir.hxx>
#include <gp_Trsf.hxx>
#include <stdexcept>
#include <string>

// Two spline endpoints meet within the source's 0.01 mm uncertainty. Clamping
// vertex tolerances to 1e-6 makes OCCT falsely report self-intersecting wires.
TopoDS_Shape toleranced_spline_prism() {
 BRep_Builder builder;
 gp_Pnt a(0,0,0), b(20,0,0), c(20,20,0), d(0,20,0);
 auto va=BRepBuilderAPI_MakeVertex(a).Vertex(), vb=BRepBuilderAPI_MakeVertex(b).Vertex();
 auto vc=BRepBuilderAPI_MakeVertex(c).Vertex(), vd=BRepBuilderAPI_MakeVertex(d).Vertex();
 for(auto v:{va,vb,vc,vd}) builder.UpdateVertex(v,0.01);
 auto edge=[](gp_Pnt start,gp_Pnt end,TopoDS_Vertex first,TopoDS_Vertex last,int corner) {
   TColgp_Array1OfPnt poles(1,4);
   poles(1)=start; poles(4)=end;
   poles(2)=gp_Pnt(start.X()+(end.X()-start.X())/3,start.Y()+(end.Y()-start.Y())/3,0);
   poles(3)=gp_Pnt(start.X()+2*(end.X()-start.X())/3,start.Y()+2*(end.Y()-start.Y())/3,0);
   if(corner==1) { poles(2)=gp_Pnt(21,15,0); poles(3)=gp_Pnt(21,20.005,0); }
   if(corner==2) { poles(2)=gp_Pnt(20.005,21,0); poles(3)=gp_Pnt(15,21,0); }
   TColStd_Array1OfReal knots(1,2); knots(1)=0; knots(2)=1;
   TColStd_Array1OfInteger mults(1,2); mults(1)=4; mults(2)=4;
   Handle(Geom_BSplineCurve) curve=new Geom_BSplineCurve(poles,knots,mults,3);
   return BRepBuilderAPI_MakeEdge(curve,first,last,0.,1.).Edge();
 };
 auto wire=BRepBuilderAPI_MakeWire(edge(a,b,va,vb,0),edge(b,gp_Pnt(20,20.005,0),vb,vc,1),
   edge(gp_Pnt(20.005,20,0),d,vc,vd,2),edge(d,a,vd,va,0)).Wire();
 auto face=BRepBuilderAPI_MakeFace(gp_Pln(gp_Pnt(0,0,0),gp_Dir(0,0,1)),wire,true).Face();
 return BRepPrimAPI_MakePrism(face,gp_Vec(0,0,5)).Shape();
}
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
 BRepFilletAPI_MakeFillet fillet(box);
 for(TopExp_Explorer edges(box,TopAbs_EDGE);edges.More();edges.Next())
   fillet.Add(1.0,TopoDS::Edge(edges.Current()));
 auto rounded=fillet.Shape();
 auto blind=BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(30,20,-2),gp_Dir(0,0,1)),4,3).Shape();
 write(BRepAlgoAPI_Cut(rounded,blind).Shape(),"blended-box-blind-bore-mm.step","MM");
 STEPControl_Writer toleranced; toleranced.SetTolerance(0.01);
 if(toleranced.Transfer(toleranced_spline_prism(),STEPControl_AsIs)!=IFSelect_RetDone
   || toleranced.Write((out+"/toleranced-spline-prism-mm.step").c_str())!=IFSelect_RetDone)
   throw std::runtime_error("toleranced fixture write failed");
}
