//! Prints the inputs and outputs of ONNX models.
fn main() -> anyhow::Result<()> {
    for path in std::env::args().skip(1) {
        let s = ort::session::Session::builder()?.commit_from_file(&path)?;
        println!("{path}");
        for i in s.inputs() {
            println!("  in  {} {:?}", i.name(), i.dtype());
        }
        for o in s.outputs() {
            println!("  out {} {:?}", o.name(), o.dtype());
        }
    }
    Ok(())
}
