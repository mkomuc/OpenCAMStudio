use cam_cldata::Program;

#[test]
fn inspect_simulated_slope() {
    let program: Program = serde_json::from_slice(&std::fs::read("/private/tmp/opencam-outside-path.json").unwrap()).unwrap();
    let sim = cam_sim::simulate(&program, [-51.,-31.,-4.], [51.,31.,21.],
        &cam_sim::SimOptions {resolution: 0.17, tool_radius:3.}, &[]);
    let (nx,ny)=sim.field.dims();
    let mut worst=(0.,[0.;3]);
    for iy in 0..ny {for ix in 0..nx {
        let x=-51.+(ix as f64+0.5)*0.17; let y=-31.+(iy as f64+0.5)*0.17;
        if x.abs()<49. && y>=-30. && y<=-20. {
            let z=sim.field.sample(x,y) as f64;let d=y+40.-z;
            if d>worst.0 {worst=(d,[x,y,z]);}
        }
    }}
    println!("raw grid worst {worst:?}");
    let mesh=sim.field.to_solid_mesh(-4.);
    let mut worst_mesh=(0f32,[0f32;3],[0f32;3]);
    for (p,n) in mesh.positions.iter().zip(&mesh.normals) {
        let [x,y,z]=*p;
        if x.abs()<49. && y>=-30. && y<=-20. && n[2]>=0. {
            let d=y+40.-z;
            if d>worst_mesh.0 {worst_mesh=(d,*p,*n);}
        }
    }
    println!("all visible mesh worst {worst_mesh:?}");
    assert!(worst.0<0.02, "{worst:?}");
    assert!(worst_mesh.0<0.1,"{worst_mesh:?}");
}

#[test]
fn inspect_current_saved_project() {
    let mut app=cam_app::AppController::new(cam_app::default_machine());
    app.open_project(concat!(env!("CARGO_MANIFEST_DIR"),"/../../testfiles/3d_clearing_tool_outside.ocam")).unwrap();
    let result=app.run(&cam_toolpath::CancelToken::new());
    assert_eq!(result.completion,cam_toolpath::JobCompletion::Complete,"{:?}",result.diagnostics);
    std::fs::write("/private/tmp/opencam-current-full-path.json",serde_json::to_vec(&result.program).unwrap()).unwrap();
    let mut worst=(0f32,[0f32;3],[0f32;3],None);
    for (i,v) in result.stock_vertices.iter().enumerate() {
        let [x,y,z]=v.position;
        if x.abs()<49. && y>=-30. && y<=-20. && v.normal[2]>=0. {
            let d=y+40.-z;
            if d>worst.0 {worst=(d,v.position,v.normal,result.stock_operations[i]);}
        }
    }
    println!("current full preview worst {worst:?}");
    assert!(worst.0<0.1,"{worst:?}");
}

#[test]
fn inspect_near_side_extra_stock() {
    let program: Program = serde_json::from_slice(&std::fs::read("/private/tmp/opencam-visible-path.json").unwrap()).unwrap();
    let sim = cam_sim::simulate(&program, [-51.,-31.,-4.], [51.,31.,21.],
        &cam_sim::SimOptions {resolution:0.05,tool_radius:3.}, &[]);
    for y in [-29.,-25.,-21.] {
        println!("Y={y}, model Z={}, stock outside part at X=-50.5: {}, stock at X=-49: {}, stock at X=0: {}",
            y+40.,sim.field.sample(-50.5,y),sim.field.sample(-49.,y),sim.field.sample(0.,y));
    }
}
