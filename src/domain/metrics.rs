#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyMetric {
    Electricity,
    Gas,
    FinalEnergy,
    PrimaryEnergy,
    Ghg,
}

impl EnergyMetric {
    pub const ALL: [Self; 5] = [
        Self::Electricity,
        Self::Gas,
        Self::FinalEnergy,
        Self::PrimaryEnergy,
        Self::Ghg,
    ];

    pub fn factor(self) -> MetricFactor {
        match self {
            Self::Electricity => MetricFactor {
                label: "전기",
                gas: 0.0,
                elec: 1.0,
                unit: "MWh",
                per_area_unit: "kWh/m2",
            },
            Self::Gas => MetricFactor {
                label: "가스",
                gas: 1.0,
                elec: 0.0,
                unit: "MWh",
                per_area_unit: "kWh/m2",
            },
            Self::FinalEnergy => MetricFactor {
                label: "에너지소요량",
                gas: 1.0,
                elec: 1.0,
                unit: "MWh",
                per_area_unit: "kWh/m2",
            },
            Self::PrimaryEnergy => MetricFactor {
                label: "1차에너지소요량",
                gas: 1.1,
                elec: 2.75,
                unit: "MWh",
                per_area_unit: "kWh/m2",
            },
            Self::Ghg => MetricFactor {
                label: "온실가스",
                gas: 0.20245,
                elec: 0.45941,
                unit: "tCO2eq",
                per_area_unit: "kgCO2eq/m2",
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MetricFactor {
    pub label: &'static str,
    pub gas: f64,
    pub elec: f64,
    pub unit: &'static str,
    #[allow(dead_code)]
    pub per_area_unit: &'static str,
}

impl MetricFactor {
    pub fn per_area_value(self, gas_kwh_m2: f64, elec_kwh_m2: f64) -> f64 {
        gas_kwh_m2 * self.gas + elec_kwh_m2 * self.elec
    }
}
