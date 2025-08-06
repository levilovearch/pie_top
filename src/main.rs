use dotenv::dotenv;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::env;
use std::fs::File;
use std::io::{Read, Write};

use tokio::sync::Mutex;
use std::time::Duration;
use std::error::Error;
use chrono::Utc;

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use serde::Deserialize;

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
    #[serde(rename = "inCash")]
    in_cash: f64,
}

#[derive(Debug, Deserialize, Clone)]
#[derive(serde::Serialize)]
struct ResultDetails {
    #[serde(rename = "priceAvgInvestedValue")]
    price_avg_invested_value: f64,
    #[serde(rename = "priceAvgValue")]
    price_avg_value: f64,
    #[serde(rename = "priceAvgResult")]
    price_avg_result: f64,
    #[serde(rename = "priceAvgResultCoef")]
    price_avg_result_coef: f64,
}

#[derive(Debug, Deserialize, Clone)]
struct PieDetail {
    settings: Setting
}

#[derive(Debug, Deserialize, Clone)]
struct Setting {
    #[serde(rename = "creationDate")]
    creation_date: f64,
}

#[derive(Debug, Clone)]
struct TotalValuePoint {
    timestamp: f64, // Unix timestamp in seconds
    total_value: f64,
}

struct PieTopApp {
    pies: Arc<Mutex<HashMap<usize, Pie>>>,
    token: String,
    last_update: std::time::Instant,
    update_interval: Duration,
    total_value_history: VecDeque<TotalValuePoint>,
    pie_list_height: f32, // Height allocated to pie list section
}

impl PieTopApp {
    fn new(token: String, pies: Arc<Mutex<HashMap<usize, Pie>>>) -> Self {
        Self {
            pies,
            token,
            last_update: std::time::Instant::now(),
            update_interval: Duration::from_secs(5), 
            total_value_history: VecDeque::new(),
            pie_list_height: 300.0, // Default height for pie list section
        }
    }
}

impl eframe::App for PieTopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update data periodically
        if self.last_update.elapsed() >= self.update_interval {
            let token = self.token.clone();
            let pies = self.pies.clone();
            
            // Spawn async task for fetching data
            tokio::spawn(async move {
                if let Err(e) = fetch_pies(&token, pies).await {
                    eprintln!("Failed to fetch pies: {}", e);
                }
            });
            
            self.last_update = std::time::Instant::now();
            
            // Update total value history when we fetch new data
            let pies_data = if let Ok(pies_guard) = self.pies.try_lock() {
                pies_guard.values().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            
            if !pies_data.is_empty() {
                let total_now: f64 = pies_data.iter().map(|p| p.result.price_avg_value).sum();
                let current_time = Utc::now().timestamp() as f64;
                
                // Add current total value to history
                self.total_value_history.push_back(TotalValuePoint {
                    timestamp: current_time,
                    total_value: total_now,
                });
                
                // Remove data older than 10 minutes (600 seconds)
                let ten_minutes_ago = current_time - 600.0;
                while let Some(front) = self.total_value_history.front() {
                    if front.timestamp < ten_minutes_ago {
                        self.total_value_history.pop_front();
                    } else {
                        break;
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Top bar with title and status
            ui.horizontal(|ui| {
                ui.heading("🥧 Pie Portfolio Dashboard");
                
                // Push status to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Last update: {:.0}s ago", self.last_update.elapsed().as_secs_f32()));
                    ui.separator();
                    ui.label("🔄 Auto-refresh every 30 seconds");
                });
            });
            ui.separator();

            // Try to get pies data without blocking
            let pies_data = if let Ok(pies_guard) = self.pies.try_lock() {
                pies_guard.values().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            if pies_data.is_empty() {
                ui.spinner();
                ui.label("Loading portfolio data...");
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }

            // Calculate totals
            let total_initial: f64 = pies_data.iter().map(|p| p.result.price_avg_invested_value).sum();
            let total_now: f64 = pies_data.iter().map(|p| p.result.price_avg_value).sum();
            let total_result_percent = if total_initial != 0.0 {
                (total_now - total_initial) / total_initial * 100.0
            } else {
                0.0
            };

            // Summary section
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("📊 Portfolio Summary:");
                    ui.separator();
                    ui.label(format!("Initial: ${:.2}", total_initial));
                    ui.separator();
                    ui.label(format!("Current: ${:.2}", total_now));
                    ui.separator();
                    
                    let color = if total_result_percent > 0.0 {
                        egui::Color32::GREEN
                    } else if total_result_percent < 0.0 {
                        egui::Color32::RED
                    } else {
                        egui::Color32::WHITE
                    };
                    
                    ui.colored_label(color, format!("Total Return: {:+.2}%", total_result_percent));
                });
            });

            ui.separator();

            // Create a resizable layout between pie list and chart
            let available_height = ui.available_height() - 100.0; // Leave some space for footer
            
            egui::TopBottomPanel::top("pie_list_panel")
                .resizable(true)
                .default_height(self.pie_list_height)
                .height_range(150.0..=available_height - 150.0)
                .show_inside(ui, |ui| {
                    // Pies table with scroll bar
                    ui.group(|ui| {
                        ui.label("📊 Pie Holdings");
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false]) // Don't shrink
                            .show(ui, |ui| {
                            egui::Grid::new("pies_grid")
                                .num_columns(7)
                                .spacing([10.0, 8.0])
                                .striped(true)
                                .show(ui, |ui| {
                                // Header
                                ui.heading("ID");
                                ui.heading("Initial Value");
                                ui.heading("Current Value");
                                ui.heading("Return %");
                                ui.heading("Progress %");
                                ui.heading("Annual Rate %");
                                ui.heading("Status");
                                ui.end_row();

                                // Pie rows
                                for pie in &pies_data {
                                    let result_percent = pie.result.price_avg_result_coef * 100.0;
                                    let progress = pie.progress.unwrap_or(0.0) * 100.0;
                                    let annual_rate = calculate_annual_rate(
                                        pie.result.price_avg_invested_value,
                                        pie.result.price_avg_value,
                                        pie.created_at.unwrap_or_default() as f64,
                                    );

                                    ui.label(pie.id.to_string());
                                    ui.label(format!("${:.2}", pie.result.price_avg_invested_value));
                                    ui.label(format!("${:.2}", pie.result.price_avg_value));
                                    
                                    let return_color = if result_percent > 0.0 {
                                        egui::Color32::GREEN
                                    } else if result_percent < 0.0 {
                                        egui::Color32::RED
                                    } else {
                                        egui::Color32::WHITE
                                    };
                                    ui.colored_label(return_color, format!("{:+.2}%", result_percent));
                                    
                                    ui.label(format!("{:.1}%", progress));
                                    
                                    let annual_color = if annual_rate > 0.0 {
                                        egui::Color32::GREEN
                                    } else if annual_rate < 0.0 {
                                        egui::Color32::RED
                                    } else {
                                        egui::Color32::WHITE
                                    };
                                    ui.colored_label(annual_color, format!("{:.2}%", annual_rate));
                                    
                                    ui.label(pie.status.as_deref().unwrap_or("Active"));
                                    ui.end_row();
                                }
                            });
                        });
                    });
                    
                    // Store the current height for next frame
                    self.pie_list_height = ui.min_rect().height();
                });

            // Chart section in the remaining space
            egui::CentralPanel::default().show_inside(ui, |ui| {
                // Total Value Chart
                ui.heading("📈 Total Value Change (Last 10 Minutes)");
                
                if self.total_value_history.len() >= 2 {
                    // Use relative time in minutes from now as X-axis
                    let current_time = Utc::now().timestamp() as f64;
                    let plot_points: PlotPoints = self.total_value_history
                        .iter()
                        .map(|point| {
                            let minutes_ago = (current_time - point.timestamp) / 60.0;
                            [-minutes_ago, point.total_value] // Negative so current time is on the right
                        })
                        .collect();
                    
                    let line = Line::new(plot_points)
                        .color(egui::Color32::from_rgb(70, 180, 220)) // Sky blue/greenish-blue
                        .width(2.0)
                        .name("Total Portfolio Value");
                    
                    Plot::new("total_value_plot")
                        .width(ui.available_width())
                        .height(ui.available_height() - 30.0) // Leave some space for the heading
                        .legend(egui_plot::Legend::default().position(egui_plot::Corner::LeftTop))
                        .x_axis_label("Time (Minutes Ago)")
                        .y_axis_label("Total Value ($)")
                        .include_x(-10.0) // Show 10 minutes ago
                        .include_x(0.0)   // Show current time (0 minutes ago)
                        .show_grid(false) // Disable grid
                        .show(ui, |plot_ui| {
                            plot_ui.line(line);
                        });
                } else {
                    ui.label("📊 Collecting data for chart... Need at least 2 data points");
                }
            });

        });

        // Request repaint for smooth updates
        ctx.request_repaint_after(Duration::from_millis(500));
    }
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
async fn main() -> Result<(), eframe::Error> {
    dotenv().ok();
    let token = env::var("TRADE212_API_TOKEN").expect("TRADE212_API_TOKEN must be set");
    
    // Load existing pies data
    let pies: Arc<Mutex<HashMap<usize, Pie>>> = Arc::new(Mutex::new(HashMap::new()));
    let pies_path = "pies.json";
    if let Ok(loaded_pies) = load_map(pies_path) {
        let mut pies_guard = pies.lock().await;
        *pies_guard = loaded_pies;
    }

    // Create the app
    let app = PieTopApp::new(token, pies.clone());
    
    // Set up native options for the window
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pie Portfolio Dashboard")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    // Save pies data when app closes
    let pies_for_save = pies.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1)); // Give some time for the app to start
        loop {
            std::thread::sleep(Duration::from_secs(5)); // Save every 5 seconds
            if let Ok(pies_map) = pies_for_save.try_lock() {
                if let Err(e) = save_map(&*pies_map, pies_path) {
                    eprintln!("Failed to save pies: {}", e);
                }
            }
        }
    });

    // Run the egui app
    eframe::run_native(
        "Pie Portfolio Dashboard",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}

async fn fetch_pies(token: &str, pies: Arc<Mutex<HashMap<usize, Pie>>>) -> Result<(), Box<dyn Error>> {
    let url = "https://live.trading212.com/api/v0/equity/pies"; // Replace with real Trade212 API endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("Authorization", format!("{}", token))
        .send()
        .await?;
    
    // Check the response status first
    let status = response.status();
    if !status.is_success() {
        if status == 429 {
            // Rate limited - just return without error to avoid spam
            return Ok(());
        }
        eprintln!("API Error: HTTP Status {}", status);
        let error_text = response.text().await?;
        eprintln!("API Error Response: {}", error_text);
        return Err(format!("HTTP Error: {} - {}", status, error_text).into());
    }
    
    // Get the response as text to see the actual format
    let response_text = response.text().await?;
    
    // Check if it's an error response first
    if response_text.contains("BusinessException") || response_text.contains("error") {
        return Err(format!("API Business Error: {}", response_text).into());
    }
    
    // Try to parse as Vec<Pie> first (array format)
    let pies_v = if let Ok(pies_array) = serde_json::from_str::<Vec<Pie>>(&response_text) {
        pies_array
    } else {
        // If that fails, try to parse as an object with pies
        #[derive(Deserialize)]
        struct PiesResponse {
            #[serde(flatten)]
            pies: HashMap<String, Pie>,
        }
        
        if let Ok(pies_obj) = serde_json::from_str::<PiesResponse>(&response_text) {
            pies_obj.pies.into_values().collect()
        } else {
            // If both fail, try direct object parsing
            match serde_json::from_str::<HashMap<String, Pie>>(&response_text) {
                Ok(pies_map) => pies_map.into_values().collect(),
                Err(e) => {
                    eprintln!("Failed to parse JSON as any expected format: {}", e);
                    eprintln!("Raw response: {}", response_text);
                    return Err(e.into());
                }
            }
        }
    };
    
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
    Ok(pie_detail.settings.creation_date)
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