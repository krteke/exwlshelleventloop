use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use iced_core::window::Id;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Duration;

#[allow(dead_code, unused_imports)]
#[path = "../../../src/redraw.rs"]
mod redraw;

use redraw::{Policy, Scope, Targets};

#[derive(Clone, Copy)]
enum Message {
    None,
    Window(Id),
    All,
}

fn scoped_policy() -> Policy<Message> {
    Policy::new(|message| match message {
        Message::None => Scope::None,
        Message::Window(id) => Scope::Window(*id),
        Message::All => Scope::All,
    })
}

fn scope_for_message(message: &Message) -> Scope {
    match message {
        Message::None => Scope::None,
        Message::Window(id) => Scope::Window(*id),
        Message::All => Scope::All,
    }
}

fn target_count_for_scope(scope: Scope) -> usize {
    match scope {
        Scope::None => 0,
        Scope::Window(_) => 1,
        Scope::All => usize::MAX,
    }
}

enum VecTargets {
    All,
    Windows(Vec<Id>),
}

struct VecPolicy {
    scope: Box<dyn Fn(&Message) -> Scope>,
}

impl VecPolicy {
    fn targets(&self, messages: &[Message]) -> VecTargets {
        let mut windows = Vec::new();

        for message in messages {
            match (self.scope)(message) {
                Scope::Window(id) if !windows.contains(&id) => windows.push(id),
                Scope::All => return VecTargets::All,
                Scope::Window(_) | Scope::None => {}
            }
        }

        VecTargets::Windows(windows)
    }
}

fn vec_policy() -> VecPolicy {
    VecPolicy {
        scope: Box::new(|message| match message {
            Message::None => Scope::None,
            Message::Window(id) => Scope::Window(*id),
            Message::All => Scope::All,
        }),
    }
}

fn target_count(targets: Targets) -> usize {
    match targets {
        Targets::None => 0,
        Targets::Window(_) => 1,
        Targets::All => usize::MAX,
        Targets::Windows(windows) => windows.len(),
    }
}

fn vec_target_count(targets: VecTargets) -> usize {
    match targets {
        VecTargets::All => usize::MAX,
        VecTargets::Windows(windows) => windows.len(),
    }
}

fn legacy_gate(messages: &[Message]) -> bool {
    !messages.is_empty()
}

fn bench_targets(c: &mut Criterion) {
    let ids = (0..4).map(|_| Id::unique()).collect::<Vec<_>>();
    let default_policy = Policy::<Message>::default();
    let scoped = scoped_policy();
    let dynamic_scope: Box<dyn Fn(&Message) -> Scope> = Box::new(|message| match message {
        Message::None => Scope::None,
        Message::Window(id) => Scope::Window(*id),
        Message::All => Scope::All,
    });
    let one_none = [Message::None];
    let one_window = [Message::Window(ids[0])];
    let one_all = [Message::All];
    let mut group = c.benchmark_group("redraw_policy/targets/single_message");

    group.bench_function("legacy_nonempty_gate", |b| {
        let messages = black_box(&one_none);

        b.iter(|| black_box(legacy_gate(messages)));
    });
    group.bench_function("direct_scope_classification", |b| {
        let message = black_box(&one_window[0]);

        b.iter(|| black_box(target_count_for_scope(scope_for_message(message))));
    });
    group.bench_function("dynamic_scope_classification", |b| {
        let scope = black_box(&dynamic_scope);
        let message = black_box(&one_window[0]);

        b.iter(|| black_box(target_count_for_scope(scope(message))));
    });
    group.bench_function("scope_to_target_count", |b| {
        let scope = black_box(Scope::Window(ids[0]));

        b.iter(|| black_box(target_count_for_scope(scope)));
    });
    group.bench_function("default_all", |b| {
        let policy = black_box(&default_policy);
        let messages = black_box(&one_none);

        b.iter(|| black_box(target_count(policy.targets(messages))));
    });
    group.bench_function("scoped_none", |b| {
        let policy = black_box(&scoped);
        let messages = black_box(&one_none);

        b.iter(|| black_box(target_count(policy.targets(messages))));
    });
    group.bench_function("scoped_window", |b| {
        let policy = black_box(&scoped);
        let messages = black_box(&one_window);

        b.iter(|| black_box(target_count(policy.targets(messages))));
    });
    group.bench_function("scoped_all", |b| {
        let policy = black_box(&scoped);
        let messages = black_box(&one_all);

        b.iter(|| black_box(target_count(policy.targets(messages))));
    });
    group.finish();

    let mut group = c.benchmark_group("redraw_policy/targets/message_batches");

    for message_count in [1, 4, 16, 64] {
        let same_window = vec![Message::Window(ids[0]); message_count];
        let alternating_windows = (0..message_count)
            .map(|index| Message::Window(ids[index % 2]))
            .collect::<Vec<_>>();
        let alternating_none_window = (0..message_count)
            .map(|index| {
                if index % 2 == 0 {
                    Message::None
                } else {
                    Message::Window(ids[0])
                }
            })
            .collect::<Vec<_>>();
        let global_last = (0..message_count)
            .map(|index| {
                if index + 1 == message_count {
                    Message::All
                } else {
                    Message::None
                }
            })
            .collect::<Vec<_>>();
        let label = format!("messages-{message_count}");

        for (name, messages) in [
            ("same_window", &same_window),
            ("alternating_windows", &alternating_windows),
            ("alternating_none_window", &alternating_none_window),
            ("global_last", &global_last),
        ] {
            group.bench_with_input(BenchmarkId::new(name, &label), messages, |b, input| {
                let policy = black_box(&scoped);
                let input = black_box(input);

                b.iter(|| black_box(target_count(policy.targets(input))));
            });
        }
    }

    group.finish();
}

fn bench_target_representation(c: &mut Criterion) {
    let ids = (0..2).map(|_| Id::unique()).collect::<Vec<_>>();
    let inline = scoped_policy();
    let vector = vec_policy();
    let cases = [
        ("same_window/messages-1", vec![Message::Window(ids[0])]),
        ("same_window/messages-4", vec![Message::Window(ids[0]); 4]),
        ("same_window/messages-64", vec![Message::Window(ids[0]); 64]),
        (
            "alternating_windows/messages-64",
            (0..64)
                .map(|index| Message::Window(ids[index % 2]))
                .collect(),
        ),
    ];

    for (group_name, inline_first) in [
        ("redraw_policy/targets/representation/vector_first", false),
        ("redraw_policy/targets/representation/inline_first", true),
    ] {
        let mut group = c.benchmark_group(group_name);

        for (name, messages) in &cases {
            if inline_first {
                group.bench_with_input(BenchmarkId::new("inline", name), messages, |b, input| {
                    let policy = black_box(&inline);
                    let input = black_box(input);

                    b.iter(|| black_box(target_count(policy.targets(input))));
                });
                group.bench_with_input(BenchmarkId::new("vector", name), messages, |b, input| {
                    let policy = black_box(&vector);
                    let input = black_box(input);

                    b.iter(|| black_box(vec_target_count(policy.targets(input))));
                });
            } else {
                group.bench_with_input(BenchmarkId::new("vector", name), messages, |b, input| {
                    let policy = black_box(&vector);
                    let input = black_box(input);

                    b.iter(|| black_box(vec_target_count(policy.targets(input))));
                });
                group.bench_with_input(BenchmarkId::new("inline", name), messages, |b, input| {
                    let policy = black_box(&inline);
                    let input = black_box(input);

                    b.iter(|| black_box(target_count(policy.targets(input))));
                });
            }
        }

        group.finish();
    }
}

#[derive(Clone, Copy)]
enum RefreshState {
    Wait,
    NextFrame,
}

#[derive(Clone)]
struct Surface {
    id: u64,
    refresh: RefreshState,
}

impl Surface {
    fn request_refresh(&mut self) {
        if matches!(self.refresh, RefreshState::Wait) {
            self.refresh = RefreshState::NextFrame;
        }
    }
}

#[derive(Clone)]
struct DispatchModel {
    windows: BTreeMap<Id, u64>,
    surfaces: Vec<Surface>,
    ids: Vec<Id>,
}

impl DispatchModel {
    fn new(surface_count: usize) -> Self {
        let ids = (0..surface_count).map(|_| Id::unique()).collect::<Vec<_>>();
        let windows = ids
            .iter()
            .copied()
            .enumerate()
            .map(|(index, id)| (id, index as u64))
            .collect();
        let surfaces = (0..surface_count)
            .map(|id| Surface {
                id: id as u64,
                refresh: RefreshState::Wait,
            })
            .collect();

        Self {
            windows,
            surfaces,
            ids,
        }
    }

    fn request_all(&mut self) -> usize {
        self.surfaces
            .iter_mut()
            .map(|surface| {
                surface.request_refresh();
                1
            })
            .sum()
    }

    fn request(&mut self, id: Id) -> usize {
        let Some(surface_id) = self.windows.get(&id) else {
            return 0;
        };
        let Some(surface) = self
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == *surface_id)
        else {
            return 0;
        };

        surface.request_refresh();
        1
    }
}

fn legacy_dispatch(model: &mut DispatchModel, messages: &[Message]) -> usize {
    if legacy_gate(messages) {
        model.request_all()
    } else {
        0
    }
}

fn scoped_dispatch(
    model: &mut DispatchModel,
    policy: &Policy<Message>,
    messages: &[Message],
) -> usize {
    if !legacy_gate(messages) {
        return 0;
    }

    match policy.targets(messages) {
        Targets::All => model.request_all(),
        Targets::None => 0,
        Targets::Window(_) if model.windows.len() <= 1 => model.request_all(),
        Targets::Window(id) => model.request(id),
        Targets::Windows(windows) if windows.len() >= model.windows.len() => model.request_all(),
        Targets::Windows(windows) => windows.into_iter().map(|id| model.request(id)).sum(),
    }
}

fn bench_dispatch_pair(
    group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>,
    surface_count: usize,
    model: &DispatchModel,
    messages: &[Message],
    policy: &Policy<Message>,
) {
    let label = format!("surfaces-{surface_count}/messages-{}", messages.len());

    group.bench_with_input(
        BenchmarkId::new("legacy_all", &label),
        messages,
        |b, input| {
            let input = black_box(input);

            b.iter_batched_ref(
                || model.clone(),
                |model| black_box(legacy_dispatch(model, input)),
                BatchSize::SmallInput,
            );
        },
    );
    group.bench_with_input(BenchmarkId::new("current", &label), messages, |b, input| {
        let policy = black_box(policy);
        let input = black_box(input);

        b.iter_batched_ref(
            || model.clone(),
            |model| black_box(scoped_dispatch(model, policy, input)),
            BatchSize::SmallInput,
        );
    });
}

fn bench_dispatch(c: &mut Criterion) {
    let default_policy = Policy::<Message>::default();
    let scoped = scoped_policy();

    for surface_count in [1, 2, 4] {
        let model = DispatchModel::new(surface_count);

        let mut group = c.benchmark_group(format!(
            "redraw_policy/dispatch/default/surfaces-{surface_count}"
        ));
        for message_count in [1, 4, 16, 64] {
            let messages = vec![Message::None; message_count];
            bench_dispatch_pair(
                &mut group,
                surface_count,
                &model,
                &messages,
                &default_policy,
            );
        }
        group.finish();

        let mut group = c.benchmark_group(format!(
            "redraw_policy/dispatch/none/surfaces-{surface_count}"
        ));
        for message_count in [1, 4, 16, 64] {
            let messages = vec![Message::None; message_count];
            bench_dispatch_pair(&mut group, surface_count, &model, &messages, &scoped);
        }
        group.finish();

        for (name, id) in [
            ("local_first", model.ids[0]),
            ("local_last", model.ids[surface_count - 1]),
        ] {
            let mut group = c.benchmark_group(format!(
                "redraw_policy/dispatch/{name}/surfaces-{surface_count}"
            ));
            for message_count in [1, 4, 16, 64] {
                let messages = vec![Message::Window(id); message_count];
                bench_dispatch_pair(&mut group, surface_count, &model, &messages, &scoped);
            }
            group.finish();
        }

        let mut group = c.benchmark_group(format!(
            "redraw_policy/dispatch/global_last/surfaces-{surface_count}"
        ));
        for message_count in [1, 4, 16, 64] {
            let messages = (0..message_count)
                .map(|index| {
                    if index + 1 == message_count {
                        Message::All
                    } else {
                        Message::None
                    }
                })
                .collect::<Vec<_>>();
            bench_dispatch_pair(&mut group, surface_count, &model, &messages, &scoped);
        }
        group.finish();

        if surface_count > 1 {
            let mut group = c.benchmark_group(format!(
                "redraw_policy/dispatch/two_windows/surfaces-{surface_count}"
            ));
            for message_count in [2, 4, 16, 64] {
                let messages = (0..message_count)
                    .map(|index| Message::Window(model.ids[index % 2]))
                    .collect::<Vec<_>>();
                bench_dispatch_pair(&mut group, surface_count, &model, &messages, &scoped);
            }
            group.finish();
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1))
        .sample_size(50);
    targets = bench_targets, bench_target_representation, bench_dispatch
}
criterion_main!(benches);
