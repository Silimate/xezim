//! §8.25 / §6.18: method formals, locals, and associative array keys typed
//! by a class typedef or a packed range over a class parameter or
//! localparam take the object's specialization width. They used to come out
//! one bit wide, so the DV `mem_model` stored a 32-bit word in two bytes.

fn t_lines(src: &str) -> Vec<String> {
    let sim = xezim::simulate(src, 100).expect("simulate");
    sim.output
        .iter()
        .map(|o| o.message.clone())
        .filter(|l| l.starts_with("T|"))
        .collect()
}

#[test]
fn class_param_sized_formals_locals_and_keys() {
    let expected = [
        "T|mem|4 12345678",
        "T|d32_formal|4 4 32",
        "T|d32_local|4 4 32 32",
        "T|d32_formal_val|f f",
        "T|d64_formal|8 8 64",
        "T|d64_local|8 8 64 64",
        "T|d64_formal_val|ff ff",
        "T|kd_key|ff",
        "T|kd_key|100",
        "T|k8_key|0",
        "T|k8_key|ff",
        "T|k40_key|0",
        "T|k40_key|ffffffffff",
    ];
    assert_eq!(t_lines(include_str!("class_param_sized_method_widths.sv")), expected);
}
