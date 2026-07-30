use mylib::Point;

fn main() {
    offload::init_guest!(artifact = "mylib").unwrap();
    let distance = mylib::dist(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 });
    println!("distance={distance}");
    let distance = mylib::dist(Point { x: 2.0, y: 1.0 }, Point { x: 3.0, y: 4.0 });
    println!("distance={distance}");
}
