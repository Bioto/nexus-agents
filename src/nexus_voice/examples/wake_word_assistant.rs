use nexus_voice::services::{TranscriptionHandler, TranscriptionResult, VoiceListener, VoiceListenerConfig};
use std::sync::mpsc;

/// Example implementation of a wake word assistant
struct WakeWordAssistant {
    wake_word: String,
    command_tx: mpsc::Sender<String>,
}

impl TranscriptionHandler for WakeWordAssistant {
    fn on_transcription(&mut self, result: TranscriptionResult) -> bool {
        println!("[{:.1}s] {}", result.duration_seconds, result.text);
        
        let text_lower = result.text.to_lowercase();
        if text_lower.starts_with(&self.wake_word) {
            // Extract command after wake word
            let command = result.text[self.wake_word.len()..].trim();
            println!("🎯 Wake word detected! Command: {}", command);
            
            // Send command to processing thread
            if let Err(_) = self.command_tx.send(command.to_string()) {
                return false; // Stop if receiver is gone
            }
        }
        
        true // Continue listening
    }
    
    fn on_error(&mut self, error: String) {
        eprintln!("Error: {}", error);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Using TranscriptionHandler trait
    println!("=== Example 1: Wake Word Assistant using TranscriptionHandler ===\n");
    
    let (command_tx, command_rx) = mpsc::channel();
    
    // Create handler
    let handler = WakeWordAssistant {
        wake_word: "hey google".to_string(),
        command_tx,
    };
    
    // Configure listener
    let config = VoiceListenerConfig {
        model_path: "src/nexus_voice/src/__models__/ggml-base.bin".into(),
        ..Default::default()
    };
    
    let mut listener = VoiceListener::new(config)?;
    
    // Start with handler
    listener.start_with_handler(handler)?;
    
    // Process commands in main thread
    std::thread::spawn(move || {
        while let Ok(command) = command_rx.recv() {
            println!("📋 Processing command: {}", command);
            // Add your command processing logic here
        }
    });
    
    // Keep running (in real app, you'd have proper shutdown logic)
    std::thread::sleep(std::time::Duration::from_secs(30));
    listener.stop()?;
    
    println!("\n=== Example 2: Using Channel API ===\n");
    
    // Example 2: Using channel API for more control
    let config = VoiceListenerConfig {
        model_path: "src/nexus_voice/src/__models__/ggml-base.bin".into(),
        ..Default::default()
    };
    
    let mut listener = VoiceListener::new(config)?;
    let rx = listener.start_with_channel()?;
    
    // Process transcriptions with full control
    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            println!("Received: {} (duration: {:.1}s, time: {:?})", 
                     result.text, 
                     result.duration_seconds,
                     result.timestamp);
            
            // Custom processing logic
            if result.text.to_lowercase().contains("stop listening") {
                println!("Stop command detected!");
                break;
            }
        }
    });
    
    // Run for 30 seconds
    std::thread::sleep(std::time::Duration::from_secs(30));
    listener.stop()?;
    
    println!("\n=== Example 3: Simple Callback (Original API) ===\n");
    
    // Example 3: Simple callback for basic use cases
    let config = VoiceListenerConfig {
        model_path: "src/nexus_voice/src/__models__/ggml-base.bin".into(),
        ..Default::default()
    };
    
    let mut listener = VoiceListener::new(config)?;
    listener.start(|text| {
        println!("Transcribed: {}", text);
    })?;
    
    // Run for 10 seconds
    std::thread::sleep(std::time::Duration::from_secs(10));
    listener.stop()?;
    
    Ok(())
}
