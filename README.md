# Rust Activity Monitor

A Windows-compatible terminal dashboard application for real-time system monitoring, built with Rust, Ratatui, and Crossterm.

## Features

- **Real-time Monitoring**: Samples system metrics every second
- **Historical Data**: Displays CPU/memory sparklines with 120 samples (~2 minutes of history)
- **Six Interactive Tabs**:
  1. **Overview**: CPU/memory gauges, sparklines, per-core chart, disks, and network summary
  2. **Simple**: Quick status view with gauges and text summary
  3. **System**: Detailed system information table
  4. **Disks**: Comprehensive disk usage and statistics
  5. **Network**: Wi-Fi stats and detailed network interface information
  6. **Processes**: Top 25 processes by CPU usage with process management
- **Dark Theme**: Tailwind-inspired color scheme with slate background and cyan accents
- **Process Management**: Terminate or kill processes directly from the UI
- **Windows-Specific Features**: Wi-Fi statistics via netsh integration

## Requirements

- Rust 1.75+ (edition 2021)
- Terminal with ANSI support
- Windows, Linux, or macOS

## Installation

### Build from Source

```bash
# Clone the repository
git clone https://github.com/antoncomputershare/rust_activity_monitor.git
cd rust_activity_monitor

# Build in release mode
cargo build --release

# Run the application
cargo run --release
```

## Usage

### Key Bindings

- **q** or **Esc**: Exit the application
- **Space**: Pause/resume metric sampling
- **r**: Manual refresh (when not paused)
- **←/→**: Switch between tabs
- **↑/↓**: Navigate process list (in Processes tab)
- **k**: Send terminate signal to selected process
- **K**: Send kill signal to selected process

### Tabs Overview

#### 1. Overview Tab
- CPU gauge and historical sparkline
- Memory gauge and historical sparkline
- Per-core CPU bar chart (up to 16 cores)
- Disk usage table
- Top 10 network interfaces by throughput

#### 2. Simple Tab
- CPU busy percentage gauge
- Memory usage percentage gauge
- Text summary including:
  - System status tier (Light/Moderate/Busy)
  - CPU and memory usage
  - Storage information
  - Network aggregate statistics
  - Wi-Fi information (if available)

#### 3. System Tab
- Host and OS information
- System uptime
- Load averages (1/5/15 minutes)
- CPU details (brand, frequency, core count)
- Memory and swap statistics
- Process count

#### 4. Disks Tab
Detailed table with columns:
- Name, Mount point, Filesystem, Type
- Total, Used, Free space
- Read/Write rates (when available)

#### 5. Network Tab
- Wi-Fi summary line (interface, RSSI, SNR)
- Network interfaces table with:
  - Interface name
  - Download/upload rates
  - Total bytes received/transmitted
  - Error counts
  - RSSI/SNR for wireless (when available)
  - Packet drops

#### 6. Processes Tab
- Top 25 processes sorted by CPU usage
- Columns: PID, Process name, CPU%, Memory
- Interactive selection with highlight
- Process termination capabilities

## Technical Details

### Dependencies

- `anyhow`: Error handling
- `ratatui` 0.29: Terminal UI framework with palette support
- `crossterm`: Cross-platform terminal manipulation
- `sysinfo` 0.37: System information gathering

### Architecture

- **Sampling Rate**: 1 Hz (every second)
- **History Length**: 120 samples (~2 minutes)
- **Wi-Fi Polling**: Every 15 seconds (2 seconds during warmup)
- **UI Mode**: Full-screen with alternate buffer and raw mode

### Windows-Specific Implementations

1. **Wi-Fi Statistics**: Uses `netsh wlan show interfaces` to parse:
   - Interface name
   - Signal strength percentage
   - Approximate RSSI in dBm (calculated from signal %)

2. **Network Throughput**: Calculated from sysinfo byte counters using delta over time

3. **Process Management**: Uses Windows TerminateProcess API via sysinfo

4. **Load Averages**: Displays 0.00 (Windows lacks native load average)

## Performance

- Minimal CPU overhead with efficient refresh cycles
- No allocations in render loop
- State reuse for optimal performance
- Release build is optimized for production use

## License

See LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## Troubleshooting

### Terminal Display Issues

If the UI doesn't display correctly, ensure your terminal:
- Supports ANSI escape codes
- Has sufficient size (minimum recommended: 80x24)
- Is properly configured for UTF-8 encoding

### Windows-Specific Issues

- Wi-Fi stats require running on a system with a wireless adapter
- Some metrics may show "N/A" if the underlying Windows API doesn't support them
- Administrative privileges may be required to terminate certain processes

### Performance Issues

If the application feels sluggish:
- Try building with `--release` flag
- Reduce terminal window size if rendering is slow
- Check if background processes are consuming resources
