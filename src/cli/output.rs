use std::{error::Error, fmt, io, time::Duration};

use eyre::{EyreHandler, Report, Result, WrapErr as _};
use indicatif::{ProgressBar, ProgressStyle};
use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme};
use rojo_schema::Artifacts;
use tracing_error::{ErrorLayer, SpanTrace};

use tracing_subscriber::{
    filter::LevelFilter, fmt::layer, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};

struct Capture(Option<SpanTrace>);

impl EyreHandler for Capture {
    fn debug(
        &self,
        error: &(dyn Error + 'static),
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        fmt::Display::fmt(error, formatter)
    }
}

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("Schema compilation failed")]
struct Failure {
    #[source]
    source: Box<dyn Error + Send + Sync>,

    #[help]
    trace: Option<String>,
}

pub(super) fn initialize(debug: bool) -> Result<()> {
    eyre::set_hook(Box::new(move |_| {
        Box::new(Capture(debug.then(SpanTrace::capture)))
    }))
    .map_err(Report::msg)
    .wrap_err("cannot initialize diagnostic tracing")?;

    tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(
            layer()
                .with_writer(io::stderr)
                .without_time()
                .with_ansi(console::colors_enabled_stderr()),
        )
        .with(if debug {
            LevelFilter::DEBUG
        } else {
            LevelFilter::WARN
        })
        .try_init()
        .map_err(Report::msg)
        .wrap_err("cannot initialize logging")
}

pub(super) fn progress(debug: bool) -> ProgressBar {
    let progress = if debug {
        ProgressBar::hidden()
    } else {
        ProgressBar::new_spinner()
    };

    progress.set_style(ProgressStyle::default_spinner().tick_strings(&["-", "\\", "|", "/", " "]));
    progress.set_message("Compiling schemas");
    progress.enable_steady_tick(Duration::from_millis(100));

    progress
}

pub(super) fn summary(verb: &str, artifacts: &Artifacts, elapsed: Duration) {
    eprintln!(
        "{} project and model schemas in {elapsed:.2?}",
        console::style(verb).for_stderr().green().bold()
    );

    eprintln!(
        "  {} classes, {} properties, {} enums",
        artifacts.stats.classes, artifacts.stats.flattened_properties, artifacts.stats.enums
    );

    eprintln!(
        "  {} project bytes, {} model bytes",
        artifacts.stats.project_schema_bytes, artifacts.stats.model_schema_bytes
    );

    if artifacts.stats.conflicts > 0 || artifacts.stats.unclassified > 0 {
        eprintln!(
            "  {} {} source conflicts, {} unclassified items; see the coverage report",
            console::style("Warning:").for_stderr().yellow().bold(),
            artifacts.stats.conflicts,
            artifacts.stats.unclassified
        );
    }
}

pub(super) fn failure(error: Report) {
    let trace = error
        .handler()
        .downcast_ref::<Capture>()
        .and_then(|capture| capture.0.as_ref())
        .map(ToString::to_string)
        .filter(|trace| !trace.is_empty());

    let failure = Failure {
        source: error.into(),
        trace,
    };

    let theme = if console::colors_enabled_stderr() {
        GraphicalTheme::unicode()
    } else {
        GraphicalTheme::none()
    };

    let mut rendered = String::new();

    match GraphicalReportHandler::new_themed(theme).render_report(&mut rendered, &failure) {
        Ok(()) => eprint!("{rendered}"),
        Err(_) => eprintln!("{failure}: {}", failure.source),
    }
}
