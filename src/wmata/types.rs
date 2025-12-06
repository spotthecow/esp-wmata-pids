use core::fmt::Write;
use heapless::String;
use miniserde::{Deserialize, de::Visitor, make_place};

#[derive(Deserialize, defmt::Format)]
pub struct NextTrain {
    #[serde(rename = "Car")]
    pub cars: Option<TrainCar>,
    #[serde(rename = "Destination")]
    pub destination: StationName,
    #[serde(rename = "DestinationCode")]
    pub destination_code: Option<Station>,
    #[serde(rename = "DestinationName")]
    pub destination_name: Option<StationName>,
    #[serde(rename = "Line")]
    pub line: Option<LineKind>,
    #[serde(rename = "LocationCode")]
    pub location_code: Station,
    #[serde(rename = "LocationName")]
    pub location_name: StationName,
    #[serde(rename = "Min")]
    pub min: Option<Eta>,
}

impl NextTrain {
    pub fn write_debug_display<const N: usize>(&self, buf: &mut String<N>) -> core::fmt::Result {
        if let Some(line) = &self.line {
            write!(buf, "[{}] ", line.code())?;
        } else {
            write!(buf, "[  ] ")?;
        }

        if let Some(cars) = &self.cars {
            write!(buf, "({}) ", cars.to_string())?;
        }

        write!(buf, "{} ", self.destination.0)?;

        if let Some(min) = &self.min {
            write!(buf, "- {}", min.to_string())?;
        }

        Ok(())
    }
}

#[derive(Deserialize)]
pub struct NextTrainsResponse {
    #[serde(rename = "Trains")]
    pub trains: alloc::vec::Vec<NextTrain>,
}

// make_place!(PlaceNextTrainsResponse);

// impl Deserialize for NextTrainsResponse {
//     fn begin(out: &mut Option<Self>) -> &mut dyn Visitor {
//         PlaceNextTrainsResponse::new(out)
//     }
// }

// struct NextTrainsBuilder<'a> {
//     out: &'a mut Option<NextTrainsResponse>,
//     acc: Vec<NextTrain, MAX_TRAINS>,
//     elem: Option<NextTrain>,
// }

// impl Visitor for PlaceNextTrainsResponse<NextTrainsResponse> {
//     fn seq(&mut self) -> miniserde::Result<Box<dyn Seq + '_>> {
//         Ok(Box::new(NextTrainsBuilder {
//             out: &mut self.out,
//             acc: Vec::new(),
//             elem: None,
//         }))
//     }
// }

impl<'a> IntoIterator for &'a NextTrainsResponse {
    type Item = &'a NextTrain;
    type IntoIter = core::slice::Iter<'a, NextTrain>;
    fn into_iter(self) -> Self::IntoIter {
        self.trains.iter()
    }
}

// impl<'a> Seq for NextTrainsBuilder<'a> {
//     fn element(&mut self) -> miniserde::Result<&mut dyn Visitor> {
//         if let Some(v) = self.elem.take() {
//             self.acc.push(v).map_err(|_| miniserde::Error)?;
//         }
//         Ok(Deserialize::begin(&mut self.elem))
//     }

//     fn finish(&mut self) -> miniserde::Result<()> {
//         if let Some(v) = self.elem.take() {
//             self.acc.push(v).map_err(|_| miniserde::Error)?;
//         }
//         *self.out = Some(NextTrainsResponse(core::mem::take(&mut self.acc)));
//         Ok(())
//     }
// }

#[derive(defmt::Format)]
pub struct TrainCar(u8);

impl From<TrainCar> for u8 {
    fn from(value: TrainCar) -> Self {
        value.0
    }
}

impl TrainCar {
    pub fn to_string(&self) -> String<1> {
        let mut s = String::<1>::new();
        write!(s, "{}", self.0).expect("to_string should always succeed");
        s
    }
}

make_place!(TrainCarPlace);

impl Deserialize for TrainCar {
    fn begin(out: &mut Option<Self>) -> &mut dyn Visitor {
        TrainCarPlace::new(out)
    }
}

impl Visitor for TrainCarPlace<TrainCar> {
    fn string(&mut self, s: &str) -> miniserde::Result<()> {
        let value = s.parse::<u8>().map_err(|_| miniserde::Error)?;
        self.out = Some(TrainCar(value));
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Line {
    pub kind: LineKind,
    pub end_station_code: Station,
    pub start_station_code: Station,
}

#[derive(Deserialize, defmt::Format)]
pub enum LineKind {
    GN,
    BL,
    SV,
    RD,
    OR,
    YL,
    NO,
}

impl LineKind {
    pub fn name(&self) -> &'static str {
        match self {
            LineKind::GN => "green",
            LineKind::BL => "bue",
            LineKind::SV => "silver",
            LineKind::RD => "red",
            LineKind::OR => "orange",
            LineKind::YL => "yellow",
            LineKind::NO => "no passengers",
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            LineKind::GN => "GR",
            LineKind::BL => "BL",
            LineKind::SV => "SV",
            LineKind::RD => "RD",
            LineKind::OR => "OR",
            LineKind::YL => "YL",
            LineKind::NO => "NO",
        }
    }
}

#[derive(defmt::Format)]
pub enum Eta {
    Minutes(u8),
    Arriving, // ARR
    Boarding, // BRD
}

impl Eta {
    pub fn to_string(&self) -> String<4> {
        let mut s: String<4> = String::new();
        match self {
            Eta::Minutes(m) => write!(s, "{}m", m).expect("to_string should always succeed"),
            Eta::Arriving => write!(s, "ARR").expect("to_string should always succeed"),
            Eta::Boarding => write!(s, "BRD").expect("to_string should always succeed"),
        };

        s
    }
}

make_place!(PlaceEta);

impl Deserialize for Eta {
    fn begin(out: &mut Option<Self>) -> &mut dyn Visitor {
        PlaceEta::new(out)
    }
}

impl Visitor for PlaceEta<Eta> {
    fn string(&mut self, s: &str) -> miniserde::Result<()> {
        let eta = match s {
            "ARR" => Eta::Arriving,
            "BRD" => Eta::Boarding,
            _ => {
                let m = s.parse::<u8>().map_err(|_| miniserde::Error)?;
                Eta::Minutes(m)
            }
        };
        self.out = Some(eta);
        Ok(())
    }
}

#[derive(defmt::Format)]
pub struct StationName(pub String<32>);

make_place!(PlaceStationName);
impl Deserialize for StationName {
    fn begin(out: &mut Option<Self>) -> &mut dyn Visitor {
        PlaceStationName::new(out)
    }
}

impl Visitor for PlaceStationName<StationName> {
    fn string(&mut self, s: &str) -> miniserde::Result<()> {
        let mut buf: String<32> = String::new();
        buf.push_str(s).map_err(|_| miniserde::Error)?;
        self.out = Some(StationName(buf));
        Ok(())
    }
}

macro_rules! stations {
    ($($v:ident),* $(,)?) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, defmt::Format)]
        pub enum Station { $( $v ),* }

        impl Station {
            pub fn code(&self) -> &str {
                match self { $( Station::$v => stringify!($v), )* }
            }
        }
    };
}

stations! {
    A01,
    A02,
    A03,
    A04,
    A05,
    A06,
    A07,
    A08,
    A09,
    A10,
    A11,
    A12,
    A13,
    A14,
    A15,
    B01,
    B02,
    B03,
    B04,
    B05,
    B06,
    B07,
    B08,
    B09,
    B10,
    B11,
    B35,
    C01,
    C02,
    C03,
    C04,
    C05,
    C06,
    C07,
    C08,
    C09,
    C10,
    C11,
    C12,
    C13,
    C14,
    C15,
    D01,
    D02,
    D03,
    D04,
    D05,
    D06,
    D07,
    D08,
    D09,
    D10,
    D11,
    D12,
    D13,
    E01,
    E02,
    E03,
    E04,
    E05,
    E06,
    E07,
    E08,
    E09,
    E10,
    F01,
    F02,
    F03,
    F04,
    F05,
    F06,
    F07,
    F08,
    F09,
    F10,
    F11,
    G01,
    G02,
    G03,
    G04,
    G05,
    J02,
    J03,
    K01,
    K02,
    K03,
    K04,
    K05,
    K06,
    K07,
    K08,
    N01,
    N02,
    N03,
    N04,
    N06,
    N07,
    N08,
    N09,
    N10,
    N11,
    N12,
}
