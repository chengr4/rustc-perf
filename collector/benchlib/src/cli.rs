use clap::{FromArgMatches, IntoApp};

#[derive(clap::Parser, Debug)]
pub enum Args {
    /// Benchmark all benchmarks in this benchmark suite and print the results as JSON.
    Benchmark(BenchmarkArgs),
}

#[derive(clap::Parser, Debug)]
pub struct BenchmarkArgs {
    /// How many times should each benchmark be repeated.
    #[clap(long, default_value = "5")]
    pub iterations: u32,
}

pub fn parse_cli() -> anyhow::Result<Args> {
    let app = Args::into_app();

    // Set the name of the help to the current binary name
    let app = app.name(
        std::env::current_exe()?
            .file_name()
            .and_then(|s| s.to_str())
            .expect("Binary name not found"),
    );

    Ok(Args::from_arg_matches(&app.get_matches())?)
}
