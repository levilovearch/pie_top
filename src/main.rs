use dotenv::dotenv;
use std::collections::HashMap;
use std::sync::Arc;
use std::{env, result};
use std::fs::File;
use std::io::{Read, Write};

use tokio::sync::Mutex;
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
use serde::{de, Deserialize};

#[derive(Debug, Deserialize, Clone)]
#[derive(serde::Serialize)]
struct Pie {
    id: u64,
    cash: f64,
    #[serde(rename = "dividendDetails")]
    dividend_details: DividendDetails,
    result: ResultDetails,
    progress: Option<f64>,
    status: Option<String>,
    created_at: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
#[derive(serde::Serialize)]
struct DividendDetails {
    gained: f64,
    reinvested: f64,
    inCash: f64,
}

#[derive(Debug, Deserialize, Clone)]
#[derive(serde::Serialize)]
struct ResultDetails {
    priceAvgInvestedValue: f64,
    priceAvgValue: f64,
    priceAvgResult: f64,
    priceAvgResultCoef: f64,
}
#[derive(Debug, Deserialize, Clone)]
struct PieDetail {
    settings: Setting
}

#[derive(Debug, Deserialize, Clone)]
struct Setting {
    creationDate: f64,
}


fn save_map(map: &HashMap<usize, Pie>, path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(map).unwrap();
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}


fn load_map(path: &str) -> std::io::Result<HashMap<usize, Pie>> {
    let mut file = File::open(path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;
    let map: HashMap<usize, Pie> = serde_json::from_str(&data).unwrap();
    Ok(map)
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
    let pies: Arc<Mutex<HashMap<usize, Pie>>> = Arc::new(Mutex::new(HashMap::new()));
    let pies_path = "pies.json";
    if let Ok(loaded_pies) = load_map(pies_path) {
        let mut pies = pies.lock().await;
        *pies = loaded_pies;
    }
    let pies_get = pies.clone();
    // Spawn background fetch task
    tokio::spawn(async move {
        loop {
            if let Ok(_) = fetch_pies(&token, pies_get.clone()).await {
                // Successfully fetched pies
            }
            sleep(Duration::from_secs(2)).await;
        }
    });
    let pies_g = pies.clone();
    // Run the UI with the receiver
    let res = run_app(&mut terminal, pies_g).await;
    // Save pies to file
    let pies_map = pies.lock().await;
    if let Err(e) = save_map(&*pies_map, pies_path) {
        eprintln!("Failed to save pies: {}", e);
    }
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
    pies: Arc<Mutex<HashMap<usize, Pie>>>,
) -> Result<(), Box<dyn Error>> {

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
        let pies: Vec<Pie> = {
            let pies_map = pies.lock().await;
            pies_map.values().cloned().collect()
        };

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
                    let annual_rate = calculate_annual_rate(
                        pie.result.priceAvgInvestedValue,
                        pie.result.priceAvgValue,
                        pie.created_at.unwrap_or_default() as f64,
                    );

                    let content = format!(
                        "ID: {} | Initial: {:.2} | Now: {:.2} | Result: {:+.2}% | Progress: {:.1}% | Annual Rate: {:.2}%",
                        pie.id,
                        pie.result.priceAvgInvestedValue,
                        pie.result.priceAvgValue,
                        result_percent,
                        progress,
                        annual_rate,
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
                },
            )));
            let list =
                List::new(items).block(Block::default().title("Pie Status").borders(Borders::ALL));

            f.render_widget(list, chunks[0]);
        })?;
    }

    Ok(())
}

async fn fetch_pies(token: &str, pies: Arc<Mutex<HashMap<usize, Pie>>>) -> Result<(), Box<dyn Error>> {
    let url = "https://live.trading212.com/api/v0/equity/pies"; // Replace with real Trade212 API endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("Authorization", format!("{}", token))
        .send()
        .await?;
    
    let pies_v = response.json::<Vec<Pie>>().await?;
    
    for pie in pies_v {
        let pie_clone = pies.clone();
        let mut p = pie_clone.lock().await;
        let p = p.entry(pie.id as usize).or_insert(pie.clone());
        if p.created_at.is_none() {
            // If created_at is None, fetch the creation date
            if let Ok(create_date) = get_create_date(&pie, &client, token).await {
                p.created_at = Some(create_date);
            }
        }
        p.result = pie.result.clone();
    }
    Ok(())
}
async fn get_create_date(pie: &Pie, client: &reqwest::Client, token: &str) -> Result<f64, Box<dyn Error>> {
    let url = "https://live.trading212.com/api/v0/equity/pies/".to_owned() + &pie.id.to_string(); // Replace with real Trade212 API endpoint
    let response = client
        .get(url)
        .header("Authorization", format!("{}", token))
        .send()
        .await?;
    let pie_detail = response.json::<PieDetail>().await?;
    Ok(pie_detail.settings.creationDate)
}

fn calculate_annual_rate(
    initial_value: f64,
    final_value: f64,
    create_date: f64,
) -> f64 {
    if initial_value <= 0.0 || create_date <= 0.0 {
        return 0.0;
    }
    let now = chrono::Utc::now().timestamp() as f64; // Convert to years
    if now <= create_date {
        return 0.0;
    }
    ((final_value / initial_value).powf(365.0 * 86400.0 / (now - create_date)) - 1.0) * 100.0
}