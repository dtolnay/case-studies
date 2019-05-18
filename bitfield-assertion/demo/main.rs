use bitfield::*;

#[bitfield] // (1+3+4+23)%8 != 0
struct NotQuiteFourBytes {
    a: B1,
    b: B3,
    c: B4,
    d: B23,
}

fn main() {}
