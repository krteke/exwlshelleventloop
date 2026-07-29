use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Path};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Subscription, Task, Theme};
use iced_layershell::build_pattern::daemon;
use iced_layershell::redraw::Scope;
use iced_layershell::reexport::{Anchor, IcedId, Layer, NewLayerShellSettings, OutputOption};
use iced_layershell::settings::{LayerShellSettings, LayerSize, Settings, StartMode};
use iced_layershell::to_layer_message;
use std::array;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use std::time::Instant;

const MAX_SURFACES: usize = 4;

fn main() -> iced_layershell::Result {
    let config = Config::from_env().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let mode = config.mode;
    let app = daemon(
        move || Benchmark::boot(config),
        "iced-layershell-targeted-redraw-benchmark",
        Benchmark::update,
        Benchmark::view,
    )
    .subscription(Benchmark::subscription)
    .settings(Settings {
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    });

    match mode {
        Mode::All => app.run(),
        Mode::Targeted => app.redraw_scope(redraw_scope).run(),
    }
}

fn redraw_scope(message: &Message) -> Scope {
    match message {
        Message::Tick(id) => Scope::Window(*id),
        Message::Start | Message::Finish => Scope::None,
        _ => Scope::All,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    All,
    Targeted,
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::All => "all",
            Self::Targeted => "targeted",
        })
    }
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "all" => Ok(Self::All),
            "targeted" => Ok(Self::Targeted),
            _ => Err(format!(
                "invalid mode `{value}`; expected `all` or `targeted`"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Config {
    mode: Mode,
    surfaces: usize,
    messages: u64,
    interval: Duration,
    warmup: Duration,
    quads: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Targeted,
            surfaces: 4,
            messages: 300,
            interval: Duration::from_millis(20),
            warmup: Duration::from_secs(1),
            quads: 16,
        }
    }
}

impl Config {
    fn from_env() -> Result<Self, String> {
        Self::parse(std::env::args().skip(1))
    }

    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut config = Self::default();
        let mut args = args.into_iter();

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--mode" => config.mode = next_value(&mut args, &flag)?.parse()?,
                "--surfaces" => {
                    config.surfaces = parse_usize(next_value(&mut args, &flag)?, &flag)?
                }
                "--messages" => config.messages = parse_u64(next_value(&mut args, &flag)?, &flag)?,
                "--interval-ms" => {
                    config.interval =
                        Duration::from_millis(parse_u64(next_value(&mut args, &flag)?, &flag)?)
                }
                "--warmup-ms" => {
                    config.warmup =
                        Duration::from_millis(parse_u64(next_value(&mut args, &flag)?, &flag)?)
                }
                "--quads" => config.quads = parse_usize(next_value(&mut args, &flag)?, &flag)?,
                "--help" | "-h" => return Err(Self::usage().to_owned()),
                _ => return Err(format!("unknown argument `{flag}`\n\n{}", Self::usage())),
            }
        }

        if !(1..=MAX_SURFACES).contains(&config.surfaces) {
            return Err(format!(
                "`--surfaces` must be between 1 and {MAX_SURFACES}, got {}",
                config.surfaces
            ));
        }
        if config.messages == 0 || config.interval.is_zero() || config.quads == 0 {
            return Err(
                "`--messages`, `--interval-ms`, and `--quads` must be greater than zero".into(),
            );
        }

        Ok(config)
    }

    fn usage() -> &'static str {
        "Usage: targeted_redraw_benchmark [OPTIONS]\n\n\
         Options:\n\
           --mode all|targeted     Redraw policy to measure (default: targeted)\n\
           --surfaces N            Active layer surfaces, from 1 to 4 (default: 4)\n\
           --messages N            Targeted messages after warm-up (default: 300)\n\
           --interval-ms N         Interval between messages (default: 20)\n\
           --warmup-ms N           Surface initialization delay (default: 1000)\n\
           --quads N               Canvas rectangles per surface (default: 16)"
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for `{flag}`"))
}

fn parse_usize(value: String, flag: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value `{value}` for `{flag}`"))
}

fn parse_u64(value: String, flag: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value `{value}` for `{flag}`"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Warming,
    Measuring,
    Draining,
}

struct Metrics {
    enabled: AtomicBool,
    draws: [AtomicU64; MAX_SURFACES],
}

impl Metrics {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            draws: array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn begin(&self) {
        for draw in &self.draws {
            draw.store(0, Ordering::Relaxed);
        }
        self.enabled.store(true, Ordering::Release);
    }

    fn record_draw(&self, index: usize) {
        if self.enabled.load(Ordering::Acquire) {
            self.draws[index].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn finish(&self, surface_count: usize) -> Vec<u64> {
        self.enabled.store(false, Ordering::Release);
        self.draws[..surface_count]
            .iter()
            .map(|draw| draw.load(Ordering::Relaxed))
            .collect()
    }
}

struct Benchmark {
    config: Config,
    ids: Vec<IcedId>,
    generations: [u64; MAX_SURFACES],
    phase: Phase,
    sent: u64,
    started: Option<Instant>,
    metrics: Arc<Metrics>,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    Start,
    Tick(IcedId),
    Finish,
}

impl Benchmark {
    fn boot(config: Config) -> (Self, Task<Message>) {
        let ids = (0..config.surfaces)
            .map(|_| IcedId::unique())
            .collect::<Vec<_>>();
        let mut tasks = ids
            .iter()
            .copied()
            .enumerate()
            .map(|(index, id)| {
                Task::done(Message::NewLayerShell {
                    settings: surface_settings(index),
                    id,
                })
            })
            .collect::<Vec<_>>();
        let warmup = config.warmup;
        tasks.push(Task::perform(
            async move { std::thread::sleep(warmup) },
            |_| Message::Start,
        ));

        (
            Self {
                config,
                ids,
                generations: [0; MAX_SURFACES],
                phase: Phase::Warming,
                sent: 0,
                started: None,
                metrics: Arc::new(Metrics::new()),
            },
            Task::batch(tasks),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Start if self.phase == Phase::Warming => {
                self.phase = Phase::Measuring;
            }
            Message::Tick(id) if self.phase == Phase::Measuring => {
                if self.sent == 0 {
                    self.metrics.begin();
                    self.started = Some(Instant::now());
                }
                self.sent += 1;
                self.generations[0] += 1;

                if self.sent == self.config.messages {
                    self.phase = Phase::Draining;
                }

                debug_assert_eq!(id, self.ids[0]);
            }
            Message::Finish if self.phase == Phase::Draining => {
                self.report();
                return iced::exit();
            }
            _ => {}
        }

        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        match self.phase {
            Phase::Warming => Subscription::none(),
            Phase::Measuring => {
                let id = self.ids[0];
                iced::time::every(self.config.interval)
                    .with(id)
                    .map(|(id, _)| Message::Tick(id))
            }
            Phase::Draining => iced::time::every(self.config.interval).map(|_| Message::Finish),
        }
    }

    fn view(&self, window: IcedId) -> Element<'_, Message> {
        let index = self
            .ids
            .iter()
            .position(|id| *id == window)
            .unwrap_or_default();
        let workload = SurfaceWorkload {
            index,
            generation: self.generations[index],
            quads: self.config.quads,
            metrics: Arc::clone(&self.metrics),
        };

        canvas::Canvas::new(workload)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn report(&self) {
        let draws = self.metrics.finish(self.config.surfaces);
        let elapsed = self
            .started
            .map_or(Duration::ZERO, |started| started.elapsed());
        let total = draws.iter().sum::<u64>();
        let per_surface = draws
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");

        println!(
            "mode={} surfaces={} messages={} interval_ms={} quads={} elapsed_ms={}",
            self.config.mode,
            self.config.surfaces,
            self.sent,
            self.config.interval.as_millis(),
            self.config.quads,
            elapsed.as_millis(),
        );
        println!("draws_per_surface={per_surface}");
        println!("total_draws={total}");
    }
}

fn surface_settings(index: usize) -> NewLayerShellSettings {
    let anchor = match index {
        0 => Anchor::Top | Anchor::Left,
        1 => Anchor::Top | Anchor::Right,
        2 => Anchor::Bottom | Anchor::Left,
        _ => Anchor::Bottom | Anchor::Right,
    };

    NewLayerShellSettings {
        size: LayerSize::px(320, 120),
        anchor,
        layer: Layer::Top,
        output_option: OutputOption::Active,
        ..Default::default()
    }
}

struct SurfaceWorkload {
    index: usize,
    generation: u64,
    quads: usize,
    metrics: Arc<Metrics>,
}

impl canvas::Program<Message> for SurfaceWorkload {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<canvas::Geometry> {
        self.metrics.record_draw(self.index);

        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width / self.quads as f32;
        let phase = (self.generation % 120) as f32 / 120.0;

        for quad in 0..self.quads {
            let factor = quad as f32 / self.quads as f32;
            let color = Color::from_rgb(
                0.08 + phase * 0.25,
                0.12 + factor * 0.35,
                0.24 + self.index as f32 * 0.08,
            );
            let path = Path::rectangle(
                Point::new(quad as f32 * width, 0.0),
                Size::new(width + 1.0, bounds.height),
            );
            frame.fill(&path, color);
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, MAX_SURFACES, Metrics, Mode};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    #[test]
    fn config_uses_realistic_defaults() {
        assert_eq!(Config::parse([]).unwrap().mode, Mode::Targeted);
        assert_eq!(Config::parse([]).unwrap().surfaces, MAX_SURFACES);
    }

    #[test]
    fn config_parses_every_measurement_option() {
        let config = Config::parse([
            "--mode".into(),
            "all".into(),
            "--surfaces".into(),
            "2".into(),
            "--messages".into(),
            "120".into(),
            "--interval-ms".into(),
            "25".into(),
            "--warmup-ms".into(),
            "500".into(),
            "--quads".into(),
            "8".into(),
        ])
        .unwrap();

        assert_eq!(config.mode, Mode::All);
        assert_eq!(config.surfaces, 2);
        assert_eq!(config.messages, 120);
        assert_eq!(config.interval, Duration::from_millis(25));
        assert_eq!(config.warmup, Duration::from_millis(500));
        assert_eq!(config.quads, 8);
    }

    #[test]
    fn config_rejects_invalid_surface_count() {
        let error = Config::parse(["--surfaces".into(), "5".into()]).unwrap_err();

        assert!(error.contains("--surfaces"));
    }

    #[test]
    fn metrics_only_count_enabled_draws() {
        let metrics = Metrics::new();
        metrics.record_draw(0);
        metrics.begin();
        metrics.record_draw(0);
        metrics.record_draw(1);
        let draws = metrics.finish(2);
        metrics.record_draw(0);

        assert_eq!(draws, [1, 1]);
        assert!(!metrics.enabled.load(Ordering::Acquire));
    }
}
