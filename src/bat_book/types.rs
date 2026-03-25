/// How common a species is in a given region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Commonness {
    VeryCommon,
    Common,
    Uncommon,
    Rare,
    Endangered,
    Vagrant,
}

impl Commonness {
    pub fn label(self) -> &'static str {
        match self {
            Self::VeryCommon => "Very Common",
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Endangered => "Endangered",
            Self::Vagrant => "Vagrant",
        }
    }
}

/// Base bat species/family data — defined once, reused across books.
pub struct BatSpecies {
    /// Unique identifier, e.g. "vespertilionidae" or "chalinolobus_gouldii"
    pub id: &'static str,
    /// Display name, e.g. "Vesper Bats" or "Gould's Wattled Bat"
    pub name: &'static str,
    /// Scientific (binomial) name, e.g. "Chalinolobus gouldii" (empty for family-level)
    pub scientific_name: &'static str,
    /// Taxonomic family name
    pub family: &'static str,
    /// Call type abbreviation (CF, FM, QCF, CF-FM, clicks, none)
    pub call_type: &'static str,
    /// Lower bound of typical echolocation frequency range (Hz)
    pub freq_lo_hz: f64,
    /// Upper bound of typical echolocation frequency range (Hz)
    pub freq_hi_hz: f64,
    /// Short description (generic / species-level)
    pub description: &'static str,
    /// Whether this species uses echolocation (false for flying foxes etc.)
    pub echolocates: bool,
}

/// A book entry definition — references a species with optional regional overrides.
pub struct BookEntryDef {
    /// Reference to the base species data
    pub species: &'static BatSpecies,
    /// Regional commonness (None for family-level entries)
    pub commonness: Option<Commonness>,
    /// Override description for this book (None = use species default)
    pub description: Option<&'static str>,
    /// Override display name for this book (None = use species default)
    pub name: Option<&'static str>,
}

impl BookEntryDef {
    /// Materialize into a full BatBookEntry by resolving overrides.
    pub fn materialize(&self) -> BatBookEntry {
        let s = self.species;
        BatBookEntry {
            id: s.id,
            name: self.name.unwrap_or(s.name),
            scientific_name: s.scientific_name,
            family: s.family,
            call_type: s.call_type,
            freq_lo_hz: s.freq_lo_hz,
            freq_hi_hz: s.freq_hi_hz,
            description: self.description.unwrap_or(s.description),
            commonness: self.commonness,
            echolocates: s.echolocates,
        }
    }
}

/// A bat book manifest containing entries for a region.
#[derive(Clone, Debug, PartialEq)]
pub struct BatBookManifest {
    pub region: String,
    pub entries: Vec<BatBookEntry>,
}

/// A single bat book entry — materialized from species + book overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct BatBookEntry {
    /// Unique identifier, e.g. "vespertilionidae" or "chalinolobus_gouldii"
    pub id: &'static str,
    /// Display name, e.g. "Vesper Bats" or "Gould's Wattled Bat"
    pub name: &'static str,
    /// Scientific (binomial) name, e.g. "Chalinolobus gouldii" (empty for family-level)
    pub scientific_name: &'static str,
    /// Taxonomic family name
    pub family: &'static str,
    /// Call type abbreviation (CF, FM, QCF, CF-FM, clicks, none)
    pub call_type: &'static str,
    /// Lower bound of typical echolocation frequency range (Hz)
    pub freq_lo_hz: f64,
    /// Upper bound of typical echolocation frequency range (Hz)
    pub freq_hi_hz: f64,
    /// Short description
    pub description: &'static str,
    /// How common the species is in the region (None for family-level entries)
    pub commonness: Option<Commonness>,
    /// Whether this species uses echolocation
    pub echolocates: bool,
}

impl BatBookEntry {
    /// Format frequency range as "XX\u{2013}YY kHz"
    pub fn freq_range_label(&self) -> String {
        if self.freq_lo_hz == 0.0 && self.freq_hi_hz == 0.0 {
            return "\u{2014}".to_string(); // em dash for no echolocation
        }
        format!(
            "{}\u{2013}{} kHz",
            (self.freq_lo_hz / 1000.0) as u32,
            (self.freq_hi_hz / 1000.0) as u32,
        )
    }
}

/// Available bat book regions.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BatBookRegion {
    #[default]
    Global,
    Europe,
    NorthAmerica,
    Australia,
    VicAustralia,
    Africa,
    SoutheastAsia,
    SouthAsia,
    EastAsia,
    MiddleEast,
    SouthAmerica,
    CostaRica,
    Japan,
    UK,
    Antarctica,
}

impl BatBookRegion {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global (All Families)",
            Self::Europe => "Europe",
            Self::NorthAmerica => "North America",
            Self::Australia => "Australia",
            Self::VicAustralia => "VIC, Australia",
            Self::Africa => "Africa",
            Self::SoutheastAsia => "Southeast Asia",
            Self::SouthAsia => "South Asia",
            Self::EastAsia => "East Asia",
            Self::MiddleEast => "Middle East",
            Self::SouthAmerica => "South America",
            Self::CostaRica => "Costa Rica",
            Self::Japan => "Japan",
            Self::UK => "United Kingdom",
            Self::Antarctica => "Antarctica",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Europe => "Europe",
            Self::NorthAmerica => "N. America",
            Self::Australia => "Australia",
            Self::VicAustralia => "VIC, AU",
            Self::Africa => "Africa",
            Self::SoutheastAsia => "SE Asia",
            Self::SouthAsia => "S. Asia",
            Self::EastAsia => "E. Asia",
            Self::MiddleEast => "Middle East",
            Self::SouthAmerica => "S. America",
            Self::CostaRica => "Costa Rica",
            Self::Japan => "Japan",
            Self::UK => "UK",
            Self::Antarctica => "Antarctica",
        }
    }

    pub const ALL: &'static [BatBookRegion] = &[
        Self::Global,
        Self::UK,
        Self::Europe,
        Self::NorthAmerica,
        Self::CostaRica,
        Self::SouthAmerica,
        Self::Africa,
        Self::MiddleEast,
        Self::SouthAsia,
        Self::SoutheastAsia,
        Self::EastAsia,
        Self::Japan,
        Self::Australia,
        Self::VicAustralia,
        Self::Antarctica,
    ];

    pub fn storage_key(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Europe => "europe",
            Self::NorthAmerica => "north_america",
            Self::Australia => "australia",
            Self::VicAustralia => "vic_australia",
            Self::Africa => "africa",
            Self::SoutheastAsia => "southeast_asia",
            Self::SouthAsia => "south_asia",
            Self::EastAsia => "east_asia",
            Self::MiddleEast => "middle_east",
            Self::SouthAmerica => "south_america",
            Self::CostaRica => "costa_rica",
            Self::Japan => "japan",
            Self::UK => "uk",
            Self::Antarctica => "antarctica",
        }
    }

    pub fn from_storage_key(key: &str) -> Option<Self> {
        Self::ALL.iter().find(|r| r.storage_key() == key).copied()
    }
}

/// Whether the bat book is in Auto mode (resolve from file metadata) or manually pinned.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BatBookMode {
    #[default]
    Auto,
    Manual(BatBookRegion),
}

impl BatBookMode {
    pub fn storage_key(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Manual(r) => r.storage_key(),
        }
    }

    pub fn from_storage_key(key: &str) -> Self {
        if key == "auto" {
            Self::Auto
        } else {
            BatBookRegion::from_storage_key(key)
                .map(Self::Manual)
                .unwrap_or(Self::Auto)
        }
    }
}

/// Result of auto-resolving the bat book from file metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoResolved {
    /// The region chosen by auto-resolve.
    pub region: BatBookRegion,
    /// Species id matched from metadata (if found in a book).
    pub matched_species_id: Option<String>,
    /// Human-readable label for what Auto resolved to, e.g. "UK", "France", "Global".
    pub source_label: String,
}
