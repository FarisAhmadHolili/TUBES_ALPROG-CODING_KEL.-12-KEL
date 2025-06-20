use actix_cors::Cors;
use actix_files::Files; // Untuk menyajikan file statis (HTML, CSS)
use actix_web::{web, App, HttpServer, Responder, HttpResponse, get, post}; // Modul Actix Web
use serde::{Serialize, Deserialize}; // Untuk serialisasi/deserialisasi JSON
use futures::stream::TryStreamExt; // Untuk memproses kursor MongoDB
use chrono::Local; // Untuk mendapatkan timestamp lokal

use mongodb::{ // Pustaka MongoDB
    bson::{doc, oid::ObjectId}, // Untuk BSON (format data MongoDB) dan ObjectId
    options::FindOptions, // Untuk opsi query MongoDB
    Client, Collection, // Klien dan Koleksi MongoDB
};

// Struct untuk merepresentasikan entri log di MongoDB
#[derive(Serialize, Deserialize, Debug, Clone)]
struct LogEntry {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    id: Option<ObjectId>, // ID unik MongoDB, opsional saat memasukkan
    timestamp: String,   // Timestamp kejadian log
    #[serde(rename = "type")] // Rename field 'log_type' menjadi 'type' di JSON
    log_type: String,    // Tipe log (misalnya "Sensor", "Reset", "Newton-Raphson")
    description: String, // Deskripsi detail log
}

// Struct untuk merepresentasikan data sensor yang masuk (hanya PPM)
#[derive(Serialize, Deserialize, Debug, Clone)]
struct SensorData {
    ppm: f64, // Nilai konsentrasi Particulate Matter (PPM)
}

// Endpoint POST untuk menambahkan log umum (misalnya dari reset dashboard atau iterasi Newton-Raphson)
#[post("/api/log")]
async fn add_log_entry(
    collection: web::Data<Collection<LogEntry>>, // Ambil instance koleksi MongoDB dari app data
    mut new_entry: web::Json<LogEntry>, // Terima entri log baru sebagai JSON
) -> impl Responder {
    // Pastikan ID tidak disetel oleh klien, biarkan MongoDB yang menggenerasinya
    new_entry.id = None;
    let result = collection.insert_one(new_entry.into_inner(), None).await;

    match result {
        Ok(res) => HttpResponse::Ok().json(res), // Respon OK dengan ID yang baru dibuat
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()), // Respon error jika gagal
    }
}

// Endpoint GET untuk mendapatkan entri log (termasuk data sensor)
#[get("/api/logs")]
async fn get_log_entries(collection: web::Data<Collection<LogEntry>>) -> impl Responder {
    // Konfigurasi opsi pencarian: urutkan berdasarkan ID menurun (terbaru dulu) dan batasi 50 entri
    let find_options = FindOptions::builder()
        .sort(doc! { "_id": -1 }) // Urutkan _id dalam urutan menurun (terbaru dulu)
        .limit(50) // Batasi hingga 50 entri terbaru
        .build();

    match collection.find(None, find_options).await {
        Ok(mut cursor) => {
            let mut logs = Vec::new();
            // Iterasi melalui kursor dan kumpulkan semua log
            while let Ok(Some(log)) = cursor.try_next().await {
                logs.push(log);
            }
            HttpResponse::Ok().json(logs) // Kirim log sebagai JSON
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()), // Respon error jika gagal
    }
}

// Endpoint POST untuk menerima data sensor dari program perantara (skrip Python/ESP32)
#[post("/api/sensor_data")]
async fn receive_sensor_data(
    collection: web::Data<Collection<LogEntry>>, // Ambil instance koleksi MongoDB
    sensor_data: web::Json<SensorData>, // Terima data sensor (PPM) sebagai JSON
) -> impl Responder {
    // Buat timestamp lokal saat ini
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    // Buat deskripsi log yang formatnya diharapkan oleh frontend
    let description = format!("PPM: {:.2}", sensor_data.ppm);

    // Buat entri log baru untuk data sensor
    let log_entry = LogEntry {
        id: None, // Biarkan MongoDB yang menggenerasi ID
        timestamp,
        log_type: "Sensor".to_string(), // Tandai sebagai 'Sensor' untuk identifikasi di frontend
        description,
    };

    let result = collection.insert_one(log_entry, None).await;

    match result {
        Ok(_) => HttpResponse::Ok().body("Sensor data received and logged successfully!"), // Respon sukses
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()), // Respon error jika gagal
    }
}

// Fungsi main aplikasi Actix Web
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // URL koneksi MongoDB
    let mongo_uri = "mongodb://localhost:27017";

    // Buat klien MongoDB dan konek
    let client = Client::with_uri_str(mongo_uri).await.expect("Gagal terhubung ke MongoDB");
    
    // Verifikasi koneksi dengan melakukan ping ke database 'admin'
    client.database("admin").run_command(doc! {"ping": 1}, None).await.expect("Gagal ping ke MongoDB");

    // Dapatkan referensi ke database 'monitoring_db' dan koleksi 'logs'
    let db = client.database("monitoring_db");
    let log_collection: Collection<LogEntry> = db.collection("logs");

    println!("✅ Berhasil terhubung ke MongoDB!");
    println!("🚀 Server starting at http://127.0.0.1:8080");

    // Konfigurasi dan jalankan server HTTP Actix Web
    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive()) // Aktifkan CORS untuk mengizinkan permintaan dari frontend
            .app_data(web::Data::new(log_collection.clone())) // Masukkan koleksi MongoDB ke data aplikasi
            .service(add_log_entry) // Daftarkan endpoint
            .service(get_log_entries) // Daftarkan endpoint
            .service(receive_sensor_data) // Daftarkan endpoint
            // Sajikan file statis dari direktori saat ini, dengan index.html sebagai file default
            .service(Files::new("/", ".").index_file("index.html")) 
    })
    .bind(("127.0.0.1", 8080))? // Bind server ke alamat IP dan port
    .run() // Jalankan server
    .await
}
