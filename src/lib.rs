use core::fmt;
use rppal::gpio::{Bias, Gpio, IoPin, Level, Mode};
use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

/// Trait representing a generic sensor with methods for reading sensor data.
pub trait Sensor<T, E> {
    /// Reads sensor data and returns a result containing either the data or an error.
    fn read_sensor_data(&mut self) -> Result<T, E>;
}

/// Struct representing the result of a DHT11 sensor reading, containing temperature and humidity.
pub struct DHT11Result {
    /// Temperature in degrees Celsius.
    pub temperature: f64,
    /// Humidity in percentage.
    pub humidity: f64,
}

/// Struct representing a DHT11 sensor controller with a GPIO pin.
pub struct DHT11Controller {
    /// GPIO pin connected to the DHT11 sensor.
    dht_pin: IoPin,
}

/// Timeout duration for collecting input during sensor communication.
const TIMEOUT_DURATION: u128 = 200; // milliseconds

impl DHT11Controller {
    /// Creates a new DHT11Controller instance with the specified GPIO pin.
    pub fn new(dht_pin: u8) -> Result<DHT11Controller, Box<dyn Error>> {
        let gpio = Gpio::new()?;
        let controller = DHT11Controller {
            dht_pin: gpio.get(dht_pin)?.into_io(Mode::Output),
        };
        return Ok(controller);
    }

    /// Collects input levels from the DHT11 sensor during communication.
    fn collect_input(&mut self) -> Vec<Level> {
        let mut last = Level::Low;
        let mut data: Vec<Level> = vec![];
        let mut start_time = Instant::now();

        loop {
            let current = self.dht_pin.read();
            data.push(current);

            if last != current {
                last = current;
                start_time = Instant::now();
            }
            if start_time.elapsed().as_millis() > TIMEOUT_DURATION {
                break;
            }
        }
        data
    }

    /// Parses the lengths of pull-up and pull-down states in the DHT11 sensor communication data.
    fn parse_data_pull_up_lengths(&mut self, data: &Vec<Level>) -> Vec<usize> {
        // Represents different states in DHT11 sensor communication protocol
        enum State {
            InitPullDown,
            InitPullUp,
            DataFirstPullDown,
            DataPullUp,
            DataPullDown,
        }

        let mut state = State::InitPullDown;
        let mut lengths: Vec<usize> = vec![];
        let mut current_length: usize = 0;

        // Transitioning from states to other states to determine the lengths
        for &current in data {
            current_length += 1;

            match state {
                State::InitPullDown => {
                    if current == Level::Low {
                        state = State::InitPullUp;
                    }
                }
                State::InitPullUp => {
                    if current == Level::High {
                        state = State::DataFirstPullDown;
                    }
                }
                State::DataFirstPullDown => {
                    if current == Level::Low {
                        state = State::DataPullUp;
                    }
                }
                State::DataPullUp => {
                    if current == Level::High {
                        current_length = 0;
                        state = State::DataPullDown;
                    }
                }
                State::DataPullDown => {
                    if current == Level::Low {
                        lengths.push(current_length);
                        state = State::DataPullUp;
                    }
                }
            }
        }
        lengths
    }

    /// Calculates bits from the pull-up lengths in the DHT11 sensor communication data.
    fn calculate_bits(&mut self, pull_up_lengths: &Vec<usize>) -> Vec<bool> {
        let mut shortest_pull_up: usize = 1000;
        let mut longest_pull_up: usize = 0;

        for length in pull_up_lengths {
            if length < &shortest_pull_up {
                shortest_pull_up = length.clone()
            }
            if length > &longest_pull_up {
                longest_pull_up = length.clone()
            }
        }

        let halfway = shortest_pull_up + (longest_pull_up - shortest_pull_up) / 2;
        let mut bits: Vec<bool> = vec![];

        for length in pull_up_lengths {
            let mut bit = false;
            if length > &halfway {
                bit = true;
            }
            bits.push(bit);
        }
        bits
    }

    /// Converts bits into bytes in the DHT11 sensor communication data.
    fn bits_to_bytes(&mut self, bits: &Vec<bool>) -> Vec<usize> {
        let mut bytes: Vec<usize> = vec![];
        let mut byte: usize = 0;

        for (i, bit) in bits.iter().enumerate() {
            byte = byte << 1;
            if *bit {
                byte = byte | 1;
            } else {
                byte = byte | 0
            }
            if (i + 1) % 8 == 0 {
                bytes.push(byte);
                byte = 0;
            }
        }
        bytes
    }

    /// Calculates the checksum from the bytes in the DHT11 sensor communication data.
    fn calculate_checksum(&mut self, bytes: &Vec<usize>) -> usize {
        bytes[0] + bytes[1] + bytes[2] + bytes[3] & 255
    }
}

/// Enum representing possible errors during DHT11 sensor communication.
#[derive(Debug)]
pub enum DHT11Error {
    /// Bit count mismatch (4 byte data + 1 byte checksum)
    MissingData,
    /// The calculated checksum (4 bytes) does not match the 1 byte validation checksum (last 1 byte)
    InvalidChecksum,
}

impl std::fmt::Display for DHT11Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingData => write!(f, "Bit count mismatch (4 byte data + 1 byte checksum)"),
            Self::InvalidChecksum => write!(f, "The calculated checksum (4 bytes) does not match the 1 byte validation checksum (last 1 byte)"),
        }
    }
}

impl Error for DHT11Error {}

impl Sensor<DHT11Result, DHT11Error> for DHT11Controller {
    fn read_sensor_data(&mut self) -> Result<DHT11Result, DHT11Error> {
        // Sending power pulse to indicate a start signal for the sensor
        self.dht_pin.set_mode(Mode::Output);
        self.dht_pin.set_high();
        thread::sleep(Duration::from_millis(50));
        self.dht_pin.set_low();
        thread::sleep(Duration::from_millis(20));

        // Receiving data
        self.dht_pin.set_mode(Mode::Input);
        self.dht_pin.set_bias(Bias::PullUp);
        let data = self.collect_input();
        let pull_up_lengths: Vec<usize> = self.parse_data_pull_up_lengths(&data);

        if pull_up_lengths.len() != 40 {
            // Bit count mismatch occurred
            return Err(DHT11Error::MissingData);
        }

        let bits = self.calculate_bits(&pull_up_lengths);
        let bytes = self.bits_to_bytes(&bits);

        let checksum = self.calculate_checksum(&bytes);
        if bytes[4] != checksum {
            // The checksum does not match the validation checksum
            return Err(DHT11Error::InvalidChecksum);
        }

        // Data was valid
        // bytes[0] : humidity    [integer]
        // bytes[1] : humidity    [decimal]
        // bytes[2] : temperature [integer]
        // bytes[3] : temperature [decimal]

        let humidity = bytes[0] as f64 + (bytes[1] as f64 / 10.0);
        let temperature = bytes[2] as f64 + (bytes[3] as f64 / 10.0);

        Ok(DHT11Result {
            temperature,
            humidity,
        })
    }
}
