use barcode_scanner::Program;

fn main() {
    let mut args = std::env::args();

    let mut program = Program::new(&mut args);

    program.set_args(&mut args).unwrap_or_else(|e| {
        program.usage();
        program.print_fail(e);
    });

    barcode_scanner::run(&program).unwrap_or_else(|e| {
        program.print_fail(e);
    });
}