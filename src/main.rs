use dotenv::dotenv;
use std::env;

use std::time::Duration;
use std::{error::Error, io};
use tokio::sync::watch;
use tokio::time::sleep;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem},
};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
struct Pie {
    id: u64,
    cash: f64,
    #[serde(rename = "dividendDetails")]
    dividend_details: DividendDetails,
    result: ResultDetails,
    progress: Option<f64>,
    status: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct DividendDetails {
    gained: f64,
    reinvested: f64,
    inCash: f64,
}

#[derive(Debug, Deserialize, Clone)]
struct ResultDetails {
    priceAvgInvestedValue: f64,
    priceAvgValue: f64,
    priceAvgResult: f64,
    priceAvgResultCoef: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let token = env::var("TRADE212_API_TOKEN")?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Watch channel for pie updates
    let (tx, rx) = watch::channel::<Vec<Pie>>(vec![]);

    // Spawn background fetch task
    tokio::spawn(async move {
        loop {
            if let Ok(pies) = fetch_pies(&token).await {
                let _ = tx.send(pies);
            }
            sleep(Duration::from_secs(2)).await;
        }
    });

    // Run the UI with the receiver
    let res = run_app(&mut terminal, rx).await;

    // Clean up
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    rx: watch::Receiver<Vec<Pie>>,
) -> Result<(), Box<dyn Error>> {
    let mut pies: Vec<Pie> = Vec::new();

    loop {
        // Check if user pressed 'q' to quit
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        // Fetch new data every 2 seconds
        if rx.has_changed().unwrap_or(false) {
            pies = rx.borrow().clone();
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Min(1)].as_ref())
                .split(f.size());
            let mut total_initial = 0.0;
            let mut total_now = 0.0;
            let mut items: Vec<ListItem> = pies
                .iter()
                .map(|pie| {
                    total_initial += pie.result.priceAvgInvestedValue;
                    total_now += pie.result.priceAvgValue;
                    let result_percent = pie.result.priceAvgResultCoef * 100.0;
                    let progress = pie.progress.unwrap_or(0.0) * 100.0;

                    let content = format!(
                        "ID: {} | Initial: {:.2} | Now: {:.2} | Result: {:+.2}% | Progress: {:.1}%",
                        pie.id,
                        pie.result.priceAvgInvestedValue,
                        pie.result.priceAvgValue,
                        result_percent,
                        progress
                    );
                    let color = if result_percent > 0.0 {
                        Color::Green
                    } else if result_percent < 0.0 {
                        Color::Red
                    } else {
                        Color::White
                    };

                    ListItem::new(content).style(Style::default().fg(color))
                })
                .collect();
            items.push(ListItem::new(format!(
                "Total Initial: {:.2} | Total Now: {:.2} | Total Result: {:+.2}%",
                total_initial,
                total_now,
                if total_initial != 0.0 {
                    (total_now - total_initial) / total_initial * 100.0
                } else {
                    0.0
                }
            )));
            let list =
                List::new(items).block(Block::default().title("Pie Status").borders(Borders::ALL));

            f.render_widget(list, chunks[0]);
        })?;
    }

    Ok(())
}

async fn fetch_pies(token: &str) -> Result<Vec<Pie>, Box<dyn Error>> {
    let url = "https://live.trading212.com/api/v0/equity/pies"; // Replace with real Trade212 API endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("Authorization", format!("{}", token))
        .send()
        .await?;
    let pies = response.json::<Vec<Pie>>().await?;
    Ok(pies)
}
