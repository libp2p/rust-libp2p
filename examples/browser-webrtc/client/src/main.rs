use browser_webrtc_example_client::App;
use leptos::*;

fn main() {
    match init_log() {
        Ok(_) => log::info!("Logging initialized"),
        Err(e) => log::error!("Error initializing logging: {:?}", e),
    }
    leptos::mount_to_body(|cx| view! { cx, <App/> })
}

// Add Logging to the Browser console
fn init_log() -> Result<(), log::SetLoggerError> {
    use fern::colors::{Color, ColoredLevelConfig};

    let colors_line = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        // we actually don't need to specify the color for debug and info, they are white by default
        .info(Color::Green)
        .debug(Color::BrightBlack)
        // depending on the terminals color scheme, this is the same as the background color
        .trace(Color::White);

    let colors_level = colors_line.info(Color::Green);

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{color_line}[{level} {target}{color_line}] {message}\x1B[0m",
                color_line = format_args!(
                    "\x1B[{}m",
                    colors_line.get_color(&record.level()).to_fg_str()
                ),
                target = record.target(),
                level = colors_level.color(record.level()),
                message = message,
            ))
        })
        .level(log::LevelFilter::Trace)
        .chain(fern::Output::call(console_log::log))
        .apply()
}
