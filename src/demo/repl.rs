use std::{
    collections::HashMap,
    error::Error,
    fmt,
    io::{self, BufRead},
};

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;

use crate::demo::{
    card::{CardView, detect_adaptive_card_view, print_card_summary},
    commands::{CommandParseError, DemoCommand, parse_command},
    help::print_help,
    history::{DemoHistory, Snapshot},
    runner::DemoRunner,
    types::{DemoBlockedOn, UserEvent},
};

#[derive(Debug)]
struct DemoReplQuit;

impl fmt::Display for DemoReplQuit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "user requested exit")
    }
}

impl Error for DemoReplQuit {}

pub struct DemoRepl {
    runner: DemoRunner,
    pending_inputs: HashMap<String, String>,
    last_output: Option<JsonValue>,
    current_card: Option<CardView>,
    history: DemoHistory,
}

impl DemoRepl {
    pub fn new(runner: DemoRunner) -> Self {
        Self {
            runner,
            pending_inputs: HashMap::new(),
            last_output: None,
            current_card: None,
            history: DemoHistory::new(),
        }
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            let blocked = self.runner.run_until_blocked();
            match blocked {
                DemoBlockedOn::Waiting { output, reason, .. } => {
                    self.current_card = detect_adaptive_card_view(&output);
                    if let Some(card) = &self.current_card {
                        let snapshot = Snapshot::new(
                            output.clone(),
                            Some(card.clone()),
                            self.pending_inputs.clone(),
                        );
                        self.history.push(snapshot);
                        self.last_output = Some(output.clone());
                        print_card_summary(card);
                        match self.command_loop() {
                            Ok(_) => {}
                            Err(err) => {
                                if err.downcast_ref::<DemoReplQuit>().is_some() {
                                    return Ok(());
                                }
                                return Err(err);
                            }
                        }
                    } else {
                        if let Some(reason) = reason {
                            println!("Waiting for input: {reason}");
                        } else {
                            println!("Flow is waiting for input (no adaptive card detected).");
                        }
                        continue;
                    }
                }
                DemoBlockedOn::Finished(output) => {
                    let output = humanize_output(&output);
                    println!("Flow finished with output:");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|_| "<invalid json>".into())
                    );
                    return Ok(());
                }
                DemoBlockedOn::Error(err) => return Err(err),
            }
        }
    }

    fn command_loop(&mut self) -> Result<()> {
        let stdin = io::stdin();
        loop {
            let mut line = String::new();
            stdin
                .lock()
                .read_line(&mut line)
                .context("read command from stdin")?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match parse_command(trimmed) {
                Ok(DemoCommand::Show) => {
                    self.display_card_summary();
                }
                Ok(DemoCommand::Json) => {
                    self.print_json();
                }
                Ok(DemoCommand::Back) => {
                    if let Some(snapshot) = self.history.go_back() {
                        self.pending_inputs = snapshot.pending_inputs.clone();
                        self.last_output = Some(snapshot.output.clone());
                        self.current_card = snapshot.card.clone();
                        println!("Restored previous blocked state.");
                        self.display_card_summary();
                    } else {
                        println!("Already at the earliest blocked state.");
                    }
                }
                Ok(DemoCommand::Help) => {
                    print_help();
                }
                Ok(DemoCommand::Quit) => {
                    return Err(DemoReplQuit.into());
                }
                Ok(DemoCommand::Input { field, value }) => {
                    if let Some(card) = &self.current_card
                        && !card.inputs.iter().any(|input| input.id == field)
                    {
                        println!(
                            "Unknown input '{field}'. Available inputs: {}",
                            self.list_input_ids(card)
                        );
                        continue;
                    }
                    self.pending_inputs.insert(field.clone(), value.clone());
                    println!("Set {field}={value}");
                }
                Ok(DemoCommand::Click { action_id }) => {
                    if let Some(card) = &self.current_card
                        && !card.actions.iter().any(|action| action.id == action_id)
                    {
                        println!(
                            "Unknown action '{action_id}'. Available actions: {}",
                            self.list_action_ids(card)
                        );
                        continue;
                    }
                    let fields = self
                        .pending_inputs
                        .iter()
                        .map(|(k, v)| (k.clone(), JsonValue::String(v.clone())))
                        .collect::<serde_json::Map<_, _>>();
                    self.pending_inputs.clear();
                    self.runner
                        .submit_user_event(UserEvent::card_submit(action_id, fields));
                    break;
                }
                Err(CommandParseError::Unknown(_)) => {
                    println!("Unknown command. See @help.");
                    print_help();
                }
                Err(err) => {
                    println!("{err}");
                    print_help();
                }
            }
        }
        Ok(())
    }

    fn display_card_summary(&self) {
        if let Some(card) = &self.current_card {
            print_card_summary(card);
            return;
        }
        println!("No adaptive card to show.");
    }

    fn print_json(&self) {
        if let Some(last_output) = &self.last_output {
            if let Ok(pretty) = serde_json::to_string_pretty(last_output) {
                println!("{pretty}");
            } else {
                println!("{}", last_output);
            }
        } else {
            println!("No output available.");
        }
    }

    fn list_input_ids(&self, card: &CardView) -> String {
        card.inputs
            .iter()
            .map(|input| input.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn list_action_ids(&self, card: &CardView) -> String {
        card.actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn humanize_output(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => {
            let mut updated = serde_json::Map::new();
            for (key, value) in map {
                if (key == "metadata" || key == "payload")
                    && let Some(bytes) = json_array_to_bytes(value)
                    && let Ok(text) = String::from_utf8(bytes)
                {
                    if let Ok(parsed) = serde_json::from_str::<JsonValue>(&text) {
                        updated.insert(key.clone(), parsed);
                        continue;
                    }
                    updated.insert(key.clone(), JsonValue::String(text));
                    continue;
                }
                updated.insert(key.clone(), humanize_output(value));
            }
            JsonValue::Object(updated)
        }
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(humanize_output).collect()),
        other => other.clone(),
    }
}

fn json_array_to_bytes(value: &JsonValue) -> Option<Vec<u8>> {
    let JsonValue::Array(items) = value else {
        return None;
    };
    let mut bytes = Vec::with_capacity(items.len());
    for item in items {
        let value = item.as_u64()?;
        if value > u8::MAX as u64 {
            return None;
        }
        bytes.push(value as u8);
    }
    Some(bytes)
}
