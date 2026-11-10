// The probe of the scale test: scripts/scale-test.mjs runs it under Callgrind, once per family and size
// (`probe <family> <n>`), and reads the families with their sizes and expected growth from `families`. Compiled with
// the "scale-probe" feature only (src/scale_probe.rs holds the workloads).
use mimicway::scale_probe::FAMILIES;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["probe", family, n] => {
            let n = n.parse().expect("size");
            if let Err(e) = mimicway::scale_probe::run(family, n) {
                eprintln!("{e}");
                std::process::exit(2);
            }
        }
        ["families"] => {
            for f in FAMILIES {
                let env: Vec<String> = f.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    f.name,
                    f.n,
                    f.growth.name(),
                    f.about,
                    env.join(" ")
                );
            }
        }
        // Run by `cargo test --features scale-probe` with no argument: nothing to count outside Callgrind.
        _ => println!(
            "scale probe: run scripts/scale-test.mjs to count the families under Callgrind"
        ),
    }
}
