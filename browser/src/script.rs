use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use blitz_dom::BaseDocument;
use blitz_traits::NodeId;
use rquickjs::{Context, Function, Runtime, function::Func};

const EXECUTION_LIMIT: Duration = Duration::from_millis(40);
const JS_MEMORY_LIMIT: usize = 8 * 1024 * 1024;
const TEXT_LIMIT: usize = 64 * 1024;

pub struct Script {
    context: Context,
    deadline: Rc<Cell<Instant>>,
}

impl Script {
    pub fn new(document: &Rc<RefCell<BaseDocument>>) -> rquickjs::Result<Self> {
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(JS_MEMORY_LIMIT);
        runtime.set_max_stack_size(256 * 1024);
        let deadline = Rc::new(Cell::new(Instant::now() + EXECUTION_LIMIT));
        let interrupt = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= interrupt.get())));
        let context = Context::full(&runtime)?;
        let query = Rc::downgrade(document);
        let get_text = query.clone();
        let set_text = query.clone();
        let get_id = query.clone();

        context.with(|cx| {
            let globals = cx.globals();
            globals.set(
                "__query",
                Func::from(move |selector: String| -> Option<String> {
                    if selector.len() > 512 {
                        return None;
                    }
                    let doc = query.upgrade()?;
                    doc.borrow()
                        .query_selector(&selector)
                        .ok()
                        .flatten()
                        .map(encode_id)
                }),
            )?;
            globals.set(
                "__getId",
                Func::from(move |name: String| -> Option<String> {
                    let doc = get_id.upgrade()?;
                    doc.borrow().get_element_by_id(&name).map(encode_id)
                }),
            )?;
            globals.set(
                "__getText",
                Func::from(move |raw: String| -> String {
                    get_text
                        .upgrade()
                        .and_then(|doc| {
                            let doc = doc.borrow();
                            doc.get_node(decode_id(&raw)?)
                                .map(|node| node.text_content())
                        })
                        .unwrap_or_default()
                }),
            )?;
            globals.set(
                "__setText",
                Func::from(move |raw: String, value: String| {
                    let Some(doc) = set_text.upgrade() else {
                        return;
                    };
                    let Some(id) = decode_id(&raw) else { return };
                    let mut doc = doc.borrow_mut();
                    if doc.get_node(id).is_some() && value.len() <= TEXT_LIMIT {
                        let mut mutator = doc.mutate();
                        mutator.remove_and_drop_all_children(id);
                        if !value.is_empty() {
                            let text = mutator.create_text_node(&value);
                            mutator.append_children(id, &[text]);
                        }
                    }
                }),
            )?;
            cx.eval::<(), _>(include_str!("dom.js"))
        })?;
        Ok(Self { context, deadline })
    }

    pub fn evaluate(&self, source: &str) {
        self.deadline.set(Instant::now() + EXECUTION_LIMIT);
        self.context.with(|cx| {
            let _ = cx.eval::<(), _>(source);
        });
    }

    pub fn ready(&self) {
        self.evaluate("globalThis.__makerReady()");
    }

    pub fn click(&self, path: &[NodeId]) -> bool {
        self.deadline.set(Instant::now() + EXECUTION_LIMIT);
        self.context
            .with(|cx| {
                let dispatch: Function = cx.globals().get("__makerClick").ok()?;
                let path = path.iter().copied().map(encode_id).collect::<Vec<_>>();
                dispatch.call::<_, bool>((path,)).ok()
            })
            .unwrap_or(false)
    }
}

fn encode_id(id: NodeId) -> String {
    id.as_u64().to_string()
}

fn decode_id(raw: &str) -> Option<NodeId> {
    raw.parse().ok().map(NodeId::from_u64)
}
