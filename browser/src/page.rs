use std::{cell::RefCell, rc::Rc, time::Instant};

use anyrender::{AntiAliasing, PaintScene, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, local_name};
use blitz_html::HtmlDocument;
use blitz_traits::{
    net::Url,
    shell::{ColorScheme, Viewport},
};
use peniko::{Color, Fill, kurbo::Rect};

use crate::{
    net::{PageRequests, Response},
    script::Script,
};

pub struct Page {
    pub url: Url,
    document: Rc<RefCell<BaseDocument>>,
    script: Option<Script>,
    _requests: PageRequests,
    opened: Instant,
    viewport: (u32, u32),
    fragment: Option<String>,
}

impl Page {
    pub fn new(response: Response, requests: PageRequests) -> Self {
        let url = response.url.clone();
        let html = response.text();
        let document = Rc::new(RefCell::new(
            HtmlDocument::from_html(
                &html,
                DocumentConfig {
                    base_url: Some(url.to_string()),
                    net_provider: Some(requests.net_provider()),
                    ..DocumentConfig::default()
                },
            )
            .into_inner(),
        ));

        let scripts = {
            let doc = document.borrow();
            doc.query_selector_all("script:not([src])")
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|id| {
                    let node = doc.get_node(id)?;
                    let kind = node
                        .attr(local_name!("type"))
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase();
                    matches!(
                        kind.as_str(),
                        "" | "text/javascript" | "application/javascript"
                    )
                    .then(|| node.text_content())
                })
                .take(32)
                .collect::<Vec<_>>()
        };
        let script = if scripts.is_empty() {
            None
        } else {
            Script::new(&document).ok().inspect(|script| {
                for source in scripts {
                    script.evaluate(&source);
                }
                script.ready();
            })
        };
        let fragment = url.fragment().map(str::to_owned);
        Self {
            url,
            document,
            script,
            _requests: requests,
            opened: Instant::now(),
            viewport: (0, 0),
            fragment,
        }
    }

    pub fn title(&self) -> String {
        let document = self.document.borrow();
        document
            .query_selector("title")
            .ok()
            .flatten()
            .and_then(|id| document.get_node(id))
            .map(|node| {
                node.text_content()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
            .chars()
            .take(90)
            .collect()
    }

    pub fn render(&mut self, width: u32, height: u32) -> Vec<u8> {
        let mut document = self.document.borrow_mut();
        if self.viewport != (width, height) {
            document.set_viewport(Viewport::new(width, height, 1.0, ColorScheme::Light));
            self.viewport = (width, height);
        }
        document.resolve(self.opened.elapsed().as_secs_f64());
        if let Some(fragment) = self.fragment.as_deref()
            && (document.scroll_to_fragment(fragment) || !document.has_pending_critical_resources())
        {
            self.fragment = None;
        }
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                scene.fill(
                    Fill::NonZero,
                    Default::default(),
                    Color::WHITE,
                    Default::default(),
                    &Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                    AntiAliasing::Enabled,
                );
                if !document.has_pending_critical_resources() {
                    blitz_paint::paint_scene(scene, &mut document, 1.0, width, height, 0, 0);
                }
            },
            width,
            height,
        )
    }

    pub fn scroll(&mut self, delta: f64) {
        self.document.borrow_mut().scroll_viewport_by(0.0, delta);
    }

    pub fn click(&mut self, x: f32, y: f32) -> Option<Url> {
        let (chain, link) = {
            let doc = self.document.borrow();
            let target = doc.element_from_point(x, y)?;
            let chain = doc.node_chain(target);
            let link = chain.iter().find_map(|&id| {
                let node = doc.get_node(id)?;
                let name: &str = node.element_data()?.name.local.as_ref();
                if name != "a" && name != "area" {
                    return None;
                }
                doc.base_url().join(node.attr(local_name!("href"))?).ok()
            });
            (chain, link)
        };
        if self
            .script
            .as_ref()
            .is_some_and(|script| script.click(&chain))
        {
            return None;
        }
        link.filter(|url| {
            url.username().is_empty()
                && url.password().is_none()
                && (matches!(url.scheme(), "http" | "https")
                    || (url.scheme() == "file" && self.url.scheme() == "file"))
        })
    }
}
