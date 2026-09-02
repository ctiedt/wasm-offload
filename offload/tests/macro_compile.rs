use offload::{AnCompatible, offload};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, AnCompatible)]
struct Point {
    x: i32,
    y: i32,
}

#[offload]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[offload(try)]
fn checked_increment(value: i32) -> i32 {
    if value == i32::MAX {
        return i32::MIN;
    }
    value + 1
}

#[offload(export = "custom-point", deny_floats)]
fn echo_point(point: Point) -> Point {
    point
}

#[offload(deny_floats)]
fn inspect_references(point: &Point, label: &str, values: &[i32]) -> i32 {
    point.x + point.y + label.len() as i32 + values.iter().sum::<i32>()
}

#[offload]
fn mutate_references(point: &mut Point, label: &mut str, values: &mut [i32]) -> i32 {
    point.x += 1;
    label.make_ascii_uppercase();
    for value in values.iter_mut() {
        *value += 1;
    }
    point.x
}

#[offload(try)]
fn try_mutate_reference(point: &mut Point) -> i32 {
    point.y += 1;
    point.y
}

#[test]
fn generated_host_signatures_are_preserved() {
    let _: fn(i32, i32) -> i32 = add;
    let _: fn(i32) -> Result<i32, offload::OffloadError> = checked_increment;
    let _: fn(Point) -> Point = echo_point;
    let _: fn(&Point, &str, &[i32]) -> i32 = inspect_references;
    let _: fn(&mut Point, &mut str, &mut [i32]) -> i32 = mutate_references;
    let _: fn(&mut Point) -> Result<i32, offload::OffloadError> = try_mutate_reference;
}
