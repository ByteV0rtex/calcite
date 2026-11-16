//! Discover → Servers: servers listed on Modrinth, with who's online and a
//! Play button that gets the right instance ready and joins.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use calcite_core::servers::{Content, Order, Server, ServerQuery, ServerResults, TAGS};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button as KitButton, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, compact_number, empty_state, icon, skeleton, thumb};
use crate::ui::pages::discover::Api;

pub struct ServersBrowser {
    query: ServerQuery,
    input: Entity<InputState>,
    results: Option<Result<ServerResults, String>>,
    loading: bool,
    /// Only servers that answered Modrinth's last check.
    online_only: bool,
    /// Servers being set up after Play.
    joining: HashSet<String>,
    /// Pinged from this computer, by address: players online, max, latency.
    /// None: it didn't answer. Missing: still asking.
    pings: HashMap<String, Option<(u32, u32, Option<i64>)>>,
    /// Addresses being pinged right now.
    asking: HashSet<String>,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ServersBrowser {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search servers"));
        let subscriptions = vec![cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.query.text = input.read(cx).value().to_string();
                this.search(Duration::from_millis(250), cx);
            }
        })];
        let mut this = ServersBrowser {
            query: ServerQuery::default(),
            input,
            results: None,
            loading: false,
            online_only: false,
            joining: HashSet::new(),
            pings: HashMap::new(),
            asking: HashSet::new(),
            pending: None,
            _subscriptions: subscriptions,
        };
        this.search(Duration::ZERO, cx);
        this
    }

    fn search(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.query.offset = 0;
        let query = self.query.clone();
        let client = cx.global::<Api>().0.clone();
        let backend = Backend::global(cx).clone();
        self.loading = true;
        self.pending = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let result = backend
                .run(async move { client.search_servers(&query).await })
                .await
                .and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.results = Some(result.map_err(|e| format!("{e:#}")));
                this.ping_new(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(results)) = &self.results else {
            return;
        };
        if self.loading || results.hits.len() >= results.total_hits {
            return;
        }
        let mut query = self.query.clone();
        query.offset = results.hits.len();
        let client = cx.global::<Api>().0.clone();
        self.loading = true;
        let task = Backend::global(cx).run(async move { client.search_servers(&query).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                if let (Ok(more), Some(Ok(results))) = (result, &mut this.results) {
                    results.hits.extend(more.hits);
                }
                this.ping_new(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Ping the listed servers this computer hasn't asked yet, all at once.
    /// Modrinth's own numbers are from its servers, not from here.
    fn ping_new(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(results)) = &self.results else {
            return;
        };
        let addresses: Vec<String> = results
            .hits
            .iter()
            .filter_map(|s| s.address().map(str::to_string))
            .filter(|a| !self.pings.contains_key(a) && !self.asking.contains(a))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if addresses.is_empty() {
            return;
        }
        self.asking.extend(addresses.iter().cloned());
        let asked = addresses.clone();
        let task = Backend::global(cx).run(async move {
            let pings = addresses.into_iter().map(|address| async move {
                let status = tokio::time::timeout(
                    Duration::from_secs(5),
                    theseus::worlds::get_server_status(&address, None),
                )
                .await;
                let result = match status {
                    Ok(Ok(status)) => {
                        let players = status.players.as_ref();
                        Some((
                            players.map(|p| p.online.max(0) as u32).unwrap_or(0),
                            players.map(|p| p.max.max(0) as u32).unwrap_or(0),
                            status.ping,
                        ))
                    }
                    _ => None,
                };
                (address, result)
            });
            futures::future::join_all(pings).await
        });
        cx.spawn(async move |this, cx| {
            let answers = task
                .await
                .unwrap_or_else(|_| asked.into_iter().map(|a| (a, None)).collect());
            let _ = this.update(cx, |this, cx| {
                for (address, answer) in answers {
                    this.asking.remove(&address);
                    this.pings.insert(address, answer);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.query.tags.iter().position(|t| t == tag) {
            self.query.tags.remove(ix);
        } else {
            self.query.tags.push(tag.to_string());
        }
        self.search(Duration::ZERO, cx);
    }

    fn play(&mut self, server: Server, cx: &mut Context<Self>) {
        self.joining.insert(server.project_id.clone());
        AppState::global(cx).update(cx, |s, cx| s.join_listed_server(server, cx));
        cx.notify();
    }

    fn row(&self, ix: usize, server: &Server, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let source = server
            .icon_url
            .as_deref()
            .and_then(|url| Images::get(url, 128, cx));

        // ● 14 / 300 · 39 ms · Europe
        let mut status = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .text_size(px(type_scale::SMALL))
            .text_color(p.text_secondary);
        let local = server.address().and_then(|a| self.pings.get(a)).copied();
        let dot = |color| div().size(px(7.0)).rounded_full().bg(color);
        let players = |online: u32, max: u32| {
            format!(
                "{} / {} online",
                compact_number(u64::from(online)),
                compact_number(u64::from(max))
            )
        };
        match (local, server.ping()) {
            // Answered this computer: its own count and latency.
            (Some(Some((online, max, ms))), _) => {
                status = status.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .child(dot(p.running))
                        .child(players(online, max)),
                );
                if let Some(ms) = ms {
                    status = status.child(
                        div()
                            .text_color(if ms < 120 {
                                p.text_secondary
                            } else {
                                p.warning
                            })
                            .child(format!("{ms} ms from you")),
                    );
                }
            }
            // No answer here; Modrinth saw it recently.
            (Some(None), Some(ping)) => {
                status = status.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .child(dot(p.warning))
                        .child(players(ping.players_online, ping.players_max))
                        .child(div().text_color(p.text_muted).child("· didn't answer you")),
                );
            }
            (Some(None), None) => {
                status = status.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .text_color(p.text_muted)
                        .child(dot(p.text_disabled))
                        .child("Offline"),
                );
            }
            // Still asking: Modrinth's count meanwhile.
            (None, ping) => {
                status = status.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .child(dot(if ping.is_some() {
                            p.running
                        } else {
                            p.text_disabled
                        }))
                        .child(match ping {
                            Some(ping) => players(ping.players_online, ping.players_max),
                            None => "Checking…".into(),
                        }),
                );
            }
        }
        if let Some(region) = server.region() {
            status = status.child(div().text_color(p.text_muted).child(region));
        }

        let needs: String = match server.content() {
            Some(Content::Vanilla {
                recommended_game_version,
                supported_game_versions,
            }) => match recommended_game_version
                .as_ref()
                .or(supported_game_versions.first())
            {
                Some(v) => format!("Minecraft {v}"),
                None => "Plain Minecraft".into(),
            },
            Some(Content::Modpack { project_name, .. }) => match project_name {
                Some(name) if *name != server.name => format!("Modpack: {name}"),
                _ => "Its own modpack".into(),
            },
            _ => "Plain Minecraft".into(),
        };
        let mut tags = div()
            .flex()
            .flex_wrap()
            .gap(space(1.5))
            .child(chip(needs, cx));
        for cat in server.categories.iter().take(4) {
            if let Some((_, label)) = TAGS.iter().find(|(key, _)| key == cat) {
                tags = tags.child(chip(label.to_string(), cx));
            }
        }

        let joining = self.joining.contains(&server.project_id);
        let can_play = server.address().is_some();
        let play = {
            let server = server.clone();
            Button::new(SharedString::from(format!("server-play-{ix}")))
                .icon("play")
                .label(if joining { "Getting ready…" } else { "Play" })
                .small()
                .primary()
                .disabled(joining || !can_play)
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.play(server.clone(), cx)),
                )
        };
        let instances: Vec<(String, SharedString)> = AppState::global(cx)
            .read(cx)
            .instances
            .iter()
            .take(20)
            .map(|i| (i.id.clone(), i.name.clone()))
            .collect();
        let (name, address, url) = (
            server.name.clone(),
            server.address().unwrap_or_default().to_string(),
            server.web_url(),
        );
        let more = KitButton::new(SharedString::from(format!("server-more-{ix}")))
            .ghost()
            .small()
            .child(icon("ellipsis", 14.0, p.text_secondary))
            .dropdown_menu(move |menu, _, _| {
                let copy = address.clone();
                let open = url.clone();
                let mut menu = menu
                    .item(PopupMenuItem::new("Copy address").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy.clone()))
                    }))
                    .item(PopupMenuItem::new("Open on modrinth.com").on_click(move |_, _, cx| {
                        cx.open_url(&open)
                    }));
                if !instances.is_empty() && !address.is_empty() {
                    menu = menu.separator().label("Add to the server list of");
                    for (id, label) in &instances {
                        let (id, name, address) = (id.clone(), name.clone(), address.clone());
                        menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                            move |_, _, cx| {
                                let (id, name, address) =
                                    (id.clone(), name.clone(), address.clone());
                                let task = Backend::global(cx).call(async move {
                                    theseus::worlds::add_server_to_instance(
                                        &id,
                                        name,
                                        address,
                                        theseus::worlds::ServerPackStatus::Prompt,
                                        None,
                                        None,
                                    )
                                    .await
                                });
                                let state = AppState::global(cx);
                                cx.spawn(async move |cx| {
                                    let result = task.await;
                                    state.update(cx, |s, cx| {
                                        match result {
                                            Ok(_) => {
                                                s.notice = Some(crate::state::Notice {
                                                    title: "Server added".into(),
                                                    body: "It's on that instance's Multiplayer screen and in its Worlds tab.".into(),
                                                    is_error: false,
                                                })
                                            }
                                            Err(error) => s.notify_error(
                                                "Couldn't add the server",
                                                format!("{error:#}"),
                                            ),
                                        }
                                        cx.notify();
                                    });
                                })
                                .detach();
                            },
                        ));
                    }
                }
                menu
            });

        div()
            .id(SharedString::from(format!("server-{ix}")))
            .flex()
            .gap(space(4.0))
            .p(space(4.0))
            .rounded(px(radius::LG))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .child(thumb(source, &server.name, 64.0, radius::MD, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(type_scale::SECTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .truncate()
                            .child(server.name.clone()),
                    )
                    .child(status)
                    .when(!server.summary.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(type_scale::BODY))
                                .text_color(p.text_secondary)
                                .line_clamp(2)
                                .child(server.summary.clone()),
                        )
                    })
                    .child(tags),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(space(1.0))
                    .child(play)
                    .child(more),
            )
            .into_any_element()
    }
}

impl Render for ServersBrowser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();

        let mut sort = div().flex().items_center().gap(space(1.0));
        for order in Order::ALL {
            let active = self.query.order == order;
            sort = sort.child(
                div()
                    .id(SharedString::from(format!(
                        "server-order-{}",
                        order.label()
                    )))
                    .h(px(28.0))
                    .px(space(2.5))
                    .flex()
                    .items_center()
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::SMALL))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.raised).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.text_color(p.text))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.query.order = order;
                        this.search(Duration::ZERO, cx);
                    }))
                    .child(order.label()),
            );
        }

        let pill = |id: SharedString, label: String, active: bool| {
            div()
                .id(id)
                .h(px(26.0))
                .px(space(2.5))
                .flex()
                .items_center()
                .rounded(px(radius::SM))
                .border_1()
                .text_size(px(type_scale::SMALL))
                .cursor_pointer()
                .when(active, |d| {
                    d.border_color(p.accent)
                        .bg(p.accent_soft)
                        .text_color(p.text)
                })
                .when(!active, |d| {
                    d.border_color(p.border)
                        .text_color(p.text_secondary)
                        .hover(|s| s.bg(p.hover))
                })
                .child(label)
        };
        let mut tags = div().flex().flex_wrap().gap(space(1.5)).child(
            pill(
                "server-online".into(),
                "Online now".into(),
                self.online_only,
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.online_only = !this.online_only;
                cx.notify();
            })),
        );
        for (key, label) in TAGS {
            let active = self.query.tags.iter().any(|t| t == key);
            tags = tags.child(
                pill(
                    SharedString::from(format!("server-tag-{key}")),
                    label.into(),
                    active,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_tag(key, cx))),
            );
        }

        let controls = div()
            .flex()
            .flex_col()
            .gap(space(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .child(
                        div().w(px(380.0)).child(
                            Input::new(&self.input)
                                .prefix(icon("search", 14.0, p.text_muted))
                                .cleanable(true)
                                .large(),
                        ),
                    )
                    .child(div().flex_1())
                    .child(sort),
            )
            .child(tags);

        let mut list = div().flex().flex_col().gap(space(2.5));
        let mut more = false;
        match &self.results {
            None => {
                for _ in 0..5 {
                    list = list.child(
                        div()
                            .flex()
                            .gap(space(4.0))
                            .p(space(4.0))
                            .rounded(px(radius::LG))
                            .border_1()
                            .border_color(p.border)
                            .child(skeleton(px(64.0), px(64.0), cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(space(2.0))
                                    .flex_1()
                                    .child(skeleton(px(200.0), px(16.0), cx))
                                    .child(skeleton(px(140.0), px(12.0), cx))
                                    .child(skeleton(gpui_kit::relative(0.8), px(12.0), cx)),
                            ),
                    );
                }
            }
            Some(Err(error)) => {
                list = list.child(empty_state(
                    "wifi-off",
                    "Couldn't load servers",
                    format!("Check your connection and try again. ({error})"),
                    None,
                    cx,
                ));
            }
            Some(Ok(results)) => {
                let hits: Vec<Server> = results
                    .hits
                    .iter()
                    .filter(|s| {
                        !self.online_only
                            || match s.address().and_then(|a| self.pings.get(a)) {
                                Some(answer) => answer.is_some(),
                                None => s.ping().is_some(),
                            }
                    })
                    .cloned()
                    .collect();
                more = results.hits.len() < results.total_hits;
                if hits.is_empty() {
                    list = list.child(empty_state(
                        "server",
                        "No servers match",
                        "Try fewer tags or other words.",
                        None,
                        cx,
                    ));
                }
                for (ix, server) in hits.iter().enumerate() {
                    list = list.child(self.row(ix, server, cx));
                }
            }
        }
        if more {
            list = list.child(
                div().flex().justify_center().py(space(4.0)).child(
                    Button::new("servers-more")
                        .label(if self.loading {
                            "Loading…"
                        } else {
                            "Show more"
                        })
                        .disabled(self.loading)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load_more(cx))),
                ),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(div().px(space(8.0)).pb(space(4.0)).child(controls))
            .child(
                div()
                    .id("servers-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(space(8.0))
                    .pb(space(10.0))
                    .child(list),
            )
    }
}
