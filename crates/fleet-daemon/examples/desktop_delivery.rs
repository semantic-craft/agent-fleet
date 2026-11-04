use fleet_daemon::DesktopDelivery;
use fleet_proto::SessionSelector;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::{
    env,
    io::{self, BufRead},
    path::{Path, PathBuf},
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: desktop_delivery SOCKET SESSION_DIRECTORY THREAD_ID; JSON operations on stdin"
                .into(),
        );
    }
    let mut delivery = DesktopDelivery::connect(
        Path::new(&args[0]),
        PathBuf::from(&args[1]),
        SessionSelector {
            thread_id: Some(args[2].clone()),
            ..Default::default()
        },
        Vec::new(),
        Duration::from_secs(10),
    )?;
    println!("{}", json!({"bound_session": delivery.session()}));
    let mut receipts = BTreeMap::new();
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let text = |key: &str| -> Result<&str, io::Error> {
            request.get(key).and_then(Value::as_str).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("missing string {key}"))
            })
        };
        let output = match text("op")? {
            "send" => {
                let id = text("delivery_id")?;
                let model = match request.get("model") {
                    None => None,
                    Some(Value::String(model)) => Some(model.as_str()),
                    _ => return Err("model must be a string when provided".into()),
                };
                match delivery.send_with_model(id, text("text")?, model) {
                    Ok(receipt) => {
                        let output = serde_json::to_value(&receipt)?;
                        receipts.insert(id.to_owned(), receipt);
                        output
                    }
                    Err(error) => {
                        json!({"delivery_id":id,"tier":error.tier(),"error":error.to_string()})
                    }
                }
            }
            "observe" => {
                let id = text("delivery_id")?;
                let receipt = receipts
                    .get(id)
                    .ok_or("delivery was not accepted by this connection")?;
                match delivery.observe(receipt) {
                    Ok(observed) => {
                        let output = serde_json::to_value(&observed)?;
                        receipts.insert(id.to_owned(), observed);
                        output
                    }
                    Err(error) => {
                        json!({"delivery_id":id,"tier":error.tier(),"error":error.to_string()})
                    }
                }
            }
            "read" => delivery.read()?,
            "interrupt" => {
                delivery.interrupt(text("turn_id")?)?;
                json!({"interrupt_requested": true})
            }
            "events" => serde_json::to_value(delivery.events()?)?,
            _ => {
                return Err(
                    "unknown operation; expected send, observe, read, interrupt, events".into(),
                )
            }
        };
        println!("{output}");
    }
    Ok(())
}
