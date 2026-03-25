//! Bat species catalog — all species and families defined once, reused across books.
//!
//! Each constant represents a base species or family. Regional bat books
//! reference these via `BookEntryDef` and can override description/name.

use super::types::BatSpecies;

// ══════════════════════════════════════════════════════════════════════════════
// Family-level entries (used by the Global book)
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Jones & Barlow (2004) JEB: Scaling of echolocation call parameters
// - Jung et al. (2014) PMC: Molossidae call design
// - Shi et al. (2024) PMC: Correlated evolution body size & echolocation
// - Jones & Rayner (1989) Springer: Horseshoe bat foraging ecology
// - Collen (2012) BioOne: Rhinolophidae & Hipposideridae comparative ecology

pub const RHINOLOPHIDAE: BatSpecies = BatSpecies {
    id: "rhinolophidae",
    name: "Horseshoe bats",
    scientific_name: "",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 120_000.0,
    description: "Constant-frequency calls; species range ~30 kHz (large) to ~112 kHz (lesser horseshoe)",
    echolocates: true,
};

pub const HIPPOSIDERIDAE: BatSpecies = BatSpecies {
    id: "hipposideridae",
    name: "Roundleaf bats",
    scientific_name: "",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 160_000.0,
    description: "CF calls; Cleotis percivalis reaches 212 kHz, the highest known bat frequency",
    echolocates: true,
};

pub const VESPERTILIONIDAE: BatSpecies = BatSpecies {
    id: "vespertilionidae",
    name: "Vesper bats",
    scientific_name: "",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 15_000.0,
    freq_hi_hz: 120_000.0,
    description: "Broadest family; FM sweeps; most species 20\u{2013}60 kHz peak",
    echolocates: true,
};

pub const MOLOSSIDAE: BatSpecies = BatSpecies {
    id: "molossidae",
    name: "Free-tailed bats",
    scientific_name: "",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 45_000.0,
    description: "Narrowband, long-duration QCF calls; 16\u{2013}44 kHz peak typical",
    echolocates: true,
};

pub const EMBALLONURIDAE: BatSpecies = BatSpecies {
    id: "emballonuridae",
    name: "Sheath-tailed bats",
    scientific_name: "",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 55_000.0,
    description: "Quasi-constant-frequency calls; some species sweep 40\u{2013}100 kHz",
    echolocates: true,
};

pub const PHYLLOSTOMIDAE: BatSpecies = BatSpecies {
    id: "phyllostomidae",
    name: "Leaf-nosed bats",
    scientific_name: "",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 120_000.0,
    description: "Low-intensity \"whispering\" bats; multi-harmonic FM calls",
    echolocates: true,
};

pub const MORMOOPIDAE: BatSpecies = BatSpecies {
    id: "mormoopidae",
    name: "Ghost-faced bats",
    scientific_name: "",
    family: "Mormoopidae",
    call_type: "CF-FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 65_000.0,
    description: "P. parnellii CF at ~63 kHz with FM sweep to ~54 kHz",
    echolocates: true,
};

pub const MINIOPTERIDAE: BatSpecies = BatSpecies {
    id: "miniopteridae",
    name: "Bent-winged bats",
    scientific_name: "",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 85_000.0,
    description: "FM-dominated calls; formerly classified within Vespertilionidae",
    echolocates: true,
};

pub const NYCTERIDAE: BatSpecies = BatSpecies {
    id: "nycteridae",
    name: "Slit-faced bats",
    scientific_name: "",
    family: "Nycteridae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 80_000.0,
    description: "Low-intensity, multi-harmonic FM calls emitted through nostrils",
    echolocates: true,
};

pub const MEGADERMATIDAE: BatSpecies = BatSpecies {
    id: "megadermatidae",
    name: "False vampires",
    scientific_name: "",
    family: "Megadermatidae",
    call_type: "FM",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 110_000.0,
    description: "Low-intensity broadband FM; large carnivorous bats",
    echolocates: true,
};

pub const PTEROPODIDAE: BatSpecies = BatSpecies {
    id: "pteropodidae",
    name: "Fruit bats",
    scientific_name: "",
    family: "Pteropodidae",
    call_type: "clicks",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 100_000.0,
    description: "Most don't echolocate; Rousettus uses tongue clicks for cave navigation",
    echolocates: false,
};

// ══════════════════════════════════════════════════════════════════════════════
// Species: Victoria, Australia
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Batica: Microbat Call Identification Assistant (Bayside, VIC)
// - SWIFFT: Insectivorous bats of Victoria
// - Milne (2002): Key to the Bat Calls of the Top End of the NT
// - Wikipedia: List of bats of Australia
// - Museums Victoria, Atlas of Living Australia

pub const CHALINOLOBUS_GOULDII: BatSpecies = BatSpecies {
    id: "chalinolobus_gouldii",
    name: "Gould's Wattled Bat",
    scientific_name: "Chalinolobus gouldii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 34_000.0,
    description: "Widespread and abundant across Australia. Roosts in tree hollows, buildings, and bat boxes. Alternating call frequencies are distinctive.",
    echolocates: true,
};

pub const CHALINOLOBUS_MORIO: BatSpecies = BatSpecies {
    id: "chalinolobus_morio",
    name: "Chocolate Wattled Bat",
    scientific_name: "Chalinolobus morio",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 53_000.0,
    description: "Common across southern Australia. Small, dark bat roosting in tree hollows and buildings. Higher frequency calls than Gould's Wattled Bat.",
    echolocates: true,
};

pub const NYCTOPHILUS_GEOFFROYI: BatSpecies = BatSpecies {
    id: "nyctophilus_geoffroyi",
    name: "Lesser Long-eared Bat",
    scientific_name: "Nyctophilus geoffroyi",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 80_000.0,
    description: "Australia's most widespread bat. Very quiet, broadband FM calls; often difficult to detect acoustically. Gleaning insectivore with large ears.",
    echolocates: true,
};

pub const AUSTRONOMUS_AUSTRALIS: BatSpecies = BatSpecies {
    id: "austronomus_australis",
    name: "White-striped Free-tailed Bat",
    scientific_name: "Austronomus australis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 15_000.0,
    description: "Australia's largest insectivorous bat. Loud, low-frequency calls audible to some humans. Fast, high-flying open-air forager.",
    echolocates: true,
};

pub const VESPADELUS_VULTURNUS: BatSpecies = BatSpecies {
    id: "vespadelus_vulturnus",
    name: "Little Forest Bat",
    scientific_name: "Vespadelus vulturnus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 53_000.0,
    description: "One of Australia's smallest bats (~4 g). Common in forests and urban areas. High-frequency calls.",
    echolocates: true,
};

pub const VESPADELUS_REGULUS: BatSpecies = BatSpecies {
    id: "vespadelus_regulus",
    name: "Southern Forest Bat",
    scientific_name: "Vespadelus regulus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 55_000.0,
    description: "Small forest bat found across southern Australia. Roosts in tree hollows. Call frequency overlaps with Little Forest Bat.",
    echolocates: true,
};

pub const NYCTOPHILUS_GOULDI: BatSpecies = BatSpecies {
    id: "nyctophilus_gouldi",
    name: "Gould's Long-eared Bat",
    scientific_name: "Nyctophilus gouldi",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 80_000.0,
    description: "Common in forests of eastern Australia. Very quiet calls, similar to Lesser Long-eared Bat. Distinguished by larger size and habitat preference.",
    echolocates: true,
};

pub const VESPADELUS_DARLINGTONI: BatSpecies = BatSpecies {
    id: "vespadelus_darlingtoni",
    name: "Large Forest Bat",
    scientific_name: "Vespadelus darlingtoni",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 46_000.0,
    description: "Largest Vespadelus species. Found in wet and dry forests of south-eastern Australia including Tasmania.",
    echolocates: true,
};

pub const MINIOPTERUS_ORIANAE_OCEANENSIS: BatSpecies = BatSpecies {
    id: "miniopterus_orianae_oceanensis",
    name: "Eastern Bent-wing Bat",
    scientific_name: "Miniopterus orianae oceanensis",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 43_000.0,
    freq_hi_hz: 48_000.0,
    description: "Cave-roosting bat found along eastern Australia. Fast, agile flier.",
    echolocates: true,
};

pub const OZIMOPS_PLANICEPS: BatSpecies = BatSpecies {
    id: "ozimops_planiceps",
    name: "Southern Free-tailed Bat",
    scientific_name: "Ozimops planiceps",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 29_000.0,
    description: "Small free-tailed bat of south-eastern Australia. Roosts in tree hollows and buildings. Rapid, direct flight.",
    echolocates: true,
};

pub const OZIMOPS_RIDEI: BatSpecies = BatSpecies {
    id: "ozimops_ridei",
    name: "Ride's Free-tailed Bat",
    scientific_name: "Ozimops ridei",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 35_000.0,
    description: "Widespread across eastern Australian coasts. Similar to Southern Free-tailed Bat but slightly higher frequency calls.",
    echolocates: true,
};

pub const FALSISTRELLUS_TASMANIENSIS: BatSpecies = BatSpecies {
    id: "falsistrellus_tasmaniensis",
    name: "Eastern Falsistrelle",
    scientific_name: "Falsistrellus tasmaniensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 34_000.0,
    freq_hi_hz: 39_000.0,
    description: "Large vesper bat of south-eastern forests. Roosts in tree hollows. Vulnerable (IUCN). Distinctive mid-range frequency calls.",
    echolocates: true,
};

pub const SCOTOREPENS_ORION: BatSpecies = BatSpecies {
    id: "scotorepens_orion",
    name: "Eastern Broad-nosed Bat",
    scientific_name: "Scotorepens orion",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 34_500.0,
    freq_hi_hz: 37_500.0,
    description: "Robust bat of south-eastern coastal forests. Narrow frequency range distinctive. Roosts in tree hollows.",
    echolocates: true,
};

pub const SCOTOREPENS_BALSTONI: BatSpecies = BatSpecies {
    id: "scotorepens_balstoni",
    name: "Inland Broad-nosed Bat",
    scientific_name: "Scotorepens balstoni",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 34_000.0,
    description: "Widespread across inland Australia. Found in drier regions. Similar frequency to Gould's Wattled Bat.",
    echolocates: true,
};

pub const MYOTIS_MACROPUS: BatSpecies = BatSpecies {
    id: "myotis_macropus",
    name: "Large-footed Myotis",
    scientific_name: "Myotis macropus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 80_000.0,
    description: "Australia's only fishing bat. Trawls water surfaces with large feet. Found near rivers, lakes, and dams. Very quiet calls.",
    echolocates: true,
};

pub const SACCOLAIMUS_FLAVIVENTRIS: BatSpecies = BatSpecies {
    id: "saccolaimus_flaviventris",
    name: "Yellow-bellied Sheathtail Bat",
    scientific_name: "Saccolaimus flaviventris",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 17_500.0,
    freq_hi_hz: 22_500.0,
    description: "Large, fast-flying bat with glossy black fur and yellow belly. Migratory. High-altitude forager.",
    echolocates: true,
};

pub const RHINOLOPHUS_MEGAPHYLLUS: BatSpecies = BatSpecies {
    id: "rhinolophus_megaphyllus",
    name: "Eastern Horseshoe Bat",
    scientific_name: "Rhinolophus megaphyllus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 67_000.0,
    freq_hi_hz: 71_000.0,
    description: "Constant-frequency echolocation using distinctive horseshoe-shaped noseleaf. Cave-roosting. Found in forests of eastern Australia.",
    echolocates: true,
};

pub const PTEROPUS_POLIOCEPHALUS: BatSpecies = BatSpecies {
    id: "pteropus_poliocephalus",
    name: "Grey-headed Flying-fox",
    scientific_name: "Pteropus poliocephalus",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Australia's largest bat (wingspan ~1 m). Does not echolocate. Camps in colonies along waterways. Vulnerable (EPBC Act). Pollinator and seed disperser.",
    echolocates: false,
};

pub const NYCTOPHILUS_MAJOR: BatSpecies = BatSpecies {
    id: "nyctophilus_major",
    name: "Greater Long-eared Bat",
    scientific_name: "Nyctophilus major",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 65_000.0,
    description: "Formerly N. timoriensis. Restricted to drier woodlands. Very quiet gleaning calls. Vulnerable.",
    echolocates: true,
};

pub const VESPADELUS_BAVERSTOCKI: BatSpecies = BatSpecies {
    id: "vespadelus_baverstocki",
    name: "Inland Forest Bat",
    scientific_name: "Vespadelus baverstocki",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 50_000.0,
    description: "Small bat of inland Australia. Restricted to semi-arid regions.",
    echolocates: true,
};

pub const SCOTEANAX_RUEPPELLII: BatSpecies = BatSpecies {
    id: "scoteanax_rueppellii",
    name: "Greater Broad-nosed Bat",
    scientific_name: "Scoteanax rueppellii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 38_000.0,
    description: "Large, robust bat of eastern coastal forests. Aggressive predator of large insects and small vertebrates.",
    echolocates: true,
};

pub const MINIOPTERUS_ORIANAE_BASSANII: BatSpecies = BatSpecies {
    id: "miniopterus_orianae_bassanii",
    name: "Southern Bent-wing Bat",
    scientific_name: "Miniopterus orianae bassanii",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 43_000.0,
    freq_hi_hz: 48_000.0,
    description: "Critically Endangered (EPBC Act). Dependent on a single maternity cave. Population critically low.",
    echolocates: true,
};

pub const NYCTOPHILUS_CORBENI: BatSpecies = BatSpecies {
    id: "nyctophilus_corbeni",
    name: "South-eastern Long-eared Bat",
    scientific_name: "Nyctophilus corbeni",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 65_000.0,
    description: "Vulnerable (EPBC Act). Extremely rare; restricted to northwest woodlands.",
    echolocates: true,
};

pub const PTEROPUS_SCAPULATUS: BatSpecies = BatSpecies {
    id: "pteropus_scapulatus",
    name: "Little Red Flying-fox",
    scientific_name: "Pteropus scapulatus",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Does not echolocate. Nomadic, following eucalypt flowering. Seasonal visitor to southern regions.",
    echolocates: false,
};

// ── Additional Australia-wide species ──────────────────────────────────────

pub const PTEROPUS_ALECTO: BatSpecies = BatSpecies {
    id: "pteropus_alecto",
    name: "Black Flying-fox",
    scientific_name: "Pteropus alecto",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Does not echolocate. Large flying-fox of tropical and subtropical northern Australia. Roosts in large colonies in mangroves, rainforest, and paperbark swamps.",
    echolocates: false,
};

pub const PTEROPUS_CONSPICILLATUS: BatSpecies = BatSpecies {
    id: "pteropus_conspicillatus",
    name: "Spectacled Flying-fox",
    scientific_name: "Pteropus conspicillatus",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Does not echolocate. Endangered (EPBC Act). Restricted to Wet Tropics and Cape York in far north Queensland. Key rainforest pollinator and seed disperser.",
    echolocates: false,
};

pub const RHINOLOPHUS_ROBERTSI: BatSpecies = BatSpecies {
    id: "rhinolophus_robertsi",
    name: "Large-eared Horseshoe Bat",
    scientific_name: "Rhinolophus robertsi",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 40_000.0,
    description: "Lowest echolocation frequency of any rhinolophid bat (~28\u{2013}34 kHz CF). Cave-roosting in warm humid caves and old mines. Restricted to tropical north Queensland. Vulnerable (EPBC Act).",
    echolocates: true,
};

pub const HIPPOSIDEROS_ATER: BatSpecies = BatSpecies {
    id: "hipposideros_ater",
    name: "Dusky Leaf-nosed Bat",
    scientific_name: "Hipposideros ater",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 128_000.0,
    freq_hi_hz: 140_000.0,
    description: "Very high-frequency CF echolocation (~130\u{2013}138 kHz). Small hipposiderid of tropical woodlands and monsoon forests in northern Australia. Roosts in caves, mines, and tree hollows.",
    echolocates: true,
};

pub const RHINONICTERIS_AURANTIA: BatSpecies = BatSpecies {
    id: "rhinonicteris_aurantia",
    name: "Orange Leaf-nosed Bat",
    scientific_name: "Rhinonicteris aurantia",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 112_000.0,
    freq_hi_hz: 122_000.0,
    description: "High-frequency CF echolocation (~114\u{2013}121 kHz) with geographic variation between Pilbara and Top End populations. Obligate cave-dweller requiring hot humid roosts. Vulnerable (EPBC Act).",
    echolocates: true,
};

pub const HIPPOSIDEROS_DIADEMA_AU: BatSpecies = BatSpecies {
    id: "hipposideros_diadema_au",
    name: "Diadem Leaf-nosed Bat",
    scientific_name: "Hipposideros diadema",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 52_000.0,
    freq_hi_hz: 68_000.0,
    description: "Largest Australian hipposiderid with striking pale shoulder markings. CF ~58\u{2013}63 kHz. Cave-roosting in tropical rainforest of far north Queensland. Long-distance forager.",
    echolocates: true,
};

pub const MACRODERMA_GIGAS: BatSpecies = BatSpecies {
    id: "macroderma_gigas",
    name: "Ghost Bat",
    scientific_name: "Macroderma gigas",
    family: "Megadermatidae",
    call_type: "FM",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 56_000.0,
    description: "Australia's only megadermatid and largest carnivorous bat. Very quiet, broadband FM calls (20\u{2013}56 kHz). Hunts vertebrate prey including other bats, lizards, and frogs. Cave-roosting. Vulnerable (EPBC Act).",
    echolocates: true,
};

pub const SACCOLAIMUS_SACCOLAIMUS: BatSpecies = BatSpecies {
    id: "saccolaimus_saccolaimus",
    name: "Bare-rumped Sheathtail Bat",
    scientific_name: "Saccolaimus saccolaimus",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 24_000.0,
    description: "Narrow-band QCF search calls averaging ~20 kHz. Large sheathtail bat of tropical woodlands in far north Queensland and Top End. Tree-hollow roosting. Critically Endangered (EPBC Act).",
    echolocates: true,
};

pub const TAPHOZOUS_GEORGIANUS: BatSpecies = BatSpecies {
    id: "taphozous_georgianus",
    name: "Common Sheathtail Bat",
    scientific_name: "Taphozous georgianus",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 28_000.0,
    description: "QCF search calls peaking ~25 kHz. Widespread across northern and western Australia. Roosts in caves, rock crevices, and abandoned mines. Fast, direct flight in open habitats.",
    echolocates: true,
};

pub const TAPHOZOUS_AUSTRALIS: BatSpecies = BatSpecies {
    id: "taphozous_australis",
    name: "Coastal Sheathtail Bat",
    scientific_name: "Taphozous australis",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 23_000.0,
    freq_hi_hz: 27_000.0,
    description: "Flat to slightly sloped QCF calls at 23\u{2013}27 kHz. Restricted to coastal caves and boulder piles along the Queensland and Northern Territory coasts. Rare and poorly known.",
    echolocates: true,
};

pub const TAPHOZOUS_TROUGHTONI: BatSpecies = BatSpecies {
    id: "taphozous_troughtoni",
    name: "Troughton's Sheathtail Bat",
    scientific_name: "Taphozous troughtoni",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 19_000.0,
    freq_hi_hz: 25_000.0,
    description: "Low-frequency QCF calls below 25 kHz. Roosts in sandstone caves and rocky escarpments in inland Queensland and western NSW. Rarely recorded; poorly known ecology.",
    echolocates: true,
};

pub const MICRONOMUS_NORFOLKENSIS: BatSpecies = BatSpecies {
    id: "micronomus_norfolkensis",
    name: "East Coast Free-tailed Bat",
    scientific_name: "Micronomus norfolkensis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 35_000.0,
    description: "QCF search calls at ~32\u{2013}35 kHz. Small free-tailed bat of coastal eastern Australia from SE Queensland to southern NSW. Roosts in tree hollows and under bark. Vulnerable (EPBC Act).",
    echolocates: true,
};

pub const CHAEREPHON_JOBENSIS: BatSpecies = BatSpecies {
    id: "chaerephon_jobensis",
    name: "Northern Free-tailed Bat",
    scientific_name: "Chaerephon jobensis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 16_000.0,
    freq_hi_hz: 25_000.0,
    description: "Very loud, low-frequency QCF calls (16\u{2013}23 kHz) often audible to humans. Large free-tailed bat of tropical northern Australia. Fast, high-altitude forager over woodland and open habitats.",
    echolocates: true,
};

pub const OZIMOPS_LUMSDENAE: BatSpecies = BatSpecies {
    id: "ozimops_lumsdenae",
    name: "Northern Free-tailed Bat",
    scientific_name: "Ozimops lumsdenae",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 21_000.0,
    freq_hi_hz: 27_000.0,
    description: "Low-frequency QCF calls peaking ~24 kHz. Largest Ozimops species. Formerly Mormopterus beccarii (in part). Widespread across tropical and subtropical northern Australia. Tree-hollow roosting.",
    echolocates: true,
};

pub const MINIOPTERUS_AUSTRALIS: BatSpecies = BatSpecies {
    id: "miniopterus_australis",
    name: "Little Bent-winged Bat",
    scientific_name: "Miniopterus australis",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 57_000.0,
    freq_hi_hz: 64_000.0,
    description: "High-frequency FM calls (57\u{2013}64 kHz). Smaller than the Eastern Bent-winged Bat. Cave-roosting in eastern Australia from Cape York to northern NSW. Often found in mixed-species colonies with Miniopterus orianae.",
    echolocates: true,
};

pub const CHALINOLOBUS_NIGROGRISEUS: BatSpecies = BatSpecies {
    id: "chalinolobus_nigrogriseus",
    name: "Hoary Wattled Bat",
    scientific_name: "Chalinolobus nigrogriseus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 26_000.0,
    freq_hi_hz: 36_000.0,
    description: "FM calls with geographic variation (26\u{2013}36 kHz). Found across northern and eastern Australia and Papua New Guinea. Roosts in tree hollows. Medium-sized wattled bat of open woodland and forest edges.",
    echolocates: true,
};

pub const VESPADELUS_TROUGHTONI: BatSpecies = BatSpecies {
    id: "vespadelus_troughtoni",
    name: "Eastern Cave Bat",
    scientific_name: "Vespadelus troughtoni",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM/QCF calls at ~49\u{2013}53 kHz. Cave-roosting bat of eastern Australia from Cape York to central NSW. Found in sandstone overhangs, caves, and mine tunnels in woodland and dry forest.",
    echolocates: true,
};

pub const VESPADELUS_FINLAYSONI: BatSpecies = BatSpecies {
    id: "vespadelus_finlaysoni",
    name: "Finlayson's Cave Bat",
    scientific_name: "Vespadelus finlaysoni",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 55_000.0,
    description: "Steep FM/QCF calls at ~53 kHz. Small bat of arid and semi-arid inland Australia. Roosts in caves, mines, rock crevices, and buildings. Tolerates very hot, dry conditions.",
    echolocates: true,
};

pub const CHALINOLOBUS_DWYERI: BatSpecies = BatSpecies {
    id: "chalinolobus_dwyeri",
    name: "Large-eared Pied Bat",
    scientific_name: "Chalinolobus dwyeri",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 48_000.0,
    description: "Broadband FM calls. Distinctive black and white fur pattern. Roosts in sandstone cliff overhangs and caves near fertile areas. Restricted to eastern Australia. Vulnerable (EPBC Act).",
    echolocates: true,
};

pub const PIPISTRELLUS_WESTRALIS: BatSpecies = BatSpecies {
    id: "pipistrellus_westralis",
    name: "Northern Pipistrelle",
    scientific_name: "Pipistrellus westralis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 42_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM calls peaking ~45\u{2013}50 kHz. One of Australia's smallest bats (~3 g). Found along northern coasts from WA through NT to Queensland. Roosts in mangroves, tree hollows, and buildings.",
    echolocates: true,
};

pub const NYCTOPHILUS_ARNHEMENSIS: BatSpecies = BatSpecies {
    id: "nyctophilus_arnhemensis",
    name: "Northern Long-eared Bat",
    scientific_name: "Nyctophilus arnhemensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 75_000.0,
    description: "Very quiet broadband FM gleaning calls, similar to other Nyctophilus. Found in tropical woodlands and monsoon forests of northern Australia (Arnhem Land, Kimberley, Cape York).",
    echolocates: true,
};

pub const NYCTOPHILUS_WALKERI: BatSpecies = BatSpecies {
    id: "nyctophilus_walkeri",
    name: "Pygmy Long-eared Bat",
    scientific_name: "Nyctophilus walkeri",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 80_000.0,
    description: "Very quiet broadband FM calls. Australia's smallest long-eared bat (~4 g). Found in sandstone escarpments and monsoon forests of the Top End and Kimberley. Gleaning insectivore.",
    echolocates: true,
};

pub const PHONISCUS_PAPUENSIS: BatSpecies = BatSpecies {
    id: "phoniscus_papuensis",
    name: "Golden-tipped Bat",
    scientific_name: "Phoniscus papuensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 155_000.0,
    description: "Extremely broadband FM calls (60\u{2013}155 kHz) for detecting stationary prey. Specialist predator of orb-weaving spiders. Roosts in abandoned bird nests. Rare along eastern Australia from Cape York to southern NSW.",
    echolocates: true,
};

// ══════════════════════════════════════════════════════════════════════════════
// Species: Europe
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Dietz, Helversen & Nill (2009): Bats of Britain, Europe and Northwest Africa
// - Russ (2012): British Bat Calls: A Guide to Species Identification
// - Barataud (2015): Acoustic Ecology of European Bats
// - Middleton, Froud & French (2014): Social Calls of the Bats of Britain and Ireland

pub const PIPISTRELLUS_PIPISTRELLUS: BatSpecies = BatSpecies {
    id: "pipistrellus_pipistrellus",
    name: "Common Pipistrelle",
    scientific_name: "Pipistrellus pipistrellus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 42_000.0,
    freq_hi_hz: 51_000.0,
    description: "Europe's most abundant bat. Characteristic frequency ~45 kHz separates it from soprano pipistrelle. Roosts in buildings; forages along edges and over water.",
    echolocates: true,
};

pub const PIPISTRELLUS_PYGMAEUS: BatSpecies = BatSpecies {
    id: "pipistrellus_pygmaeus",
    name: "Soprano Pipistrelle",
    scientific_name: "Pipistrellus pygmaeus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 51_000.0,
    freq_hi_hz: 60_000.0,
    description: "Cryptic species split from common pipistrelle in 1999. Characteristic frequency ~55 kHz. Strongly associated with riparian habitats.",
    echolocates: true,
};

pub const PIPISTRELLUS_NATHUSII: BatSpecies = BatSpecies {
    id: "pipistrellus_nathusii",
    name: "Nathusius' Pipistrelle",
    scientific_name: "Pipistrellus nathusii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 42_000.0,
    description: "Long-distance migrant; travels up to 2,000 km. Characteristic frequency ~38 kHz. Favours wetlands and riparian woodland.",
    echolocates: true,
};

pub const PIPISTRELLUS_KUHLII: BatSpecies = BatSpecies {
    id: "pipistrellus_kuhlii",
    name: "Kuhl's Pipistrelle",
    scientific_name: "Pipistrellus kuhlii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 45_000.0,
    description: "Expanding northward across Europe. Characteristic frequency ~40 kHz. Common around buildings and street lights in Mediterranean regions.",
    echolocates: true,
};

pub const MYOTIS_DAUBENTONII: BatSpecies = BatSpecies {
    id: "myotis_daubentonii",
    name: "Daubenton's Bat",
    scientific_name: "Myotis daubentonii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 32_000.0,
    freq_hi_hz: 85_000.0,
    description: "Forages low over calm water, trawling insects from the surface. Steep FM sweeps. Often seen along canals and rivers at dusk.",
    echolocates: true,
};

pub const MYOTIS_NATTERERI: BatSpecies = BatSpecies {
    id: "myotis_nattereri",
    name: "Natterer's Bat",
    scientific_name: "Myotis nattereri",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 23_000.0,
    freq_hi_hz: 115_000.0,
    description: "Very broadband FM calls with prominent harmonics. Gleaning specialist, picks prey from foliage and walls. Roosts in old buildings and tree holes.",
    echolocates: true,
};

pub const MYOTIS_MYSTACINUS: BatSpecies = BatSpecies {
    id: "myotis_mystacinus",
    name: "Whiskered Bat",
    scientific_name: "Myotis mystacinus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 32_000.0,
    freq_hi_hz: 80_000.0,
    description: "Small Myotis often found in villages and woodland edges. Very similar in call and appearance to Brandt's bat; confirmed by handling or genetics.",
    echolocates: true,
};

pub const MYOTIS_BRANDTII: BatSpecies = BatSpecies {
    id: "myotis_brandtii",
    name: "Brandt's Bat",
    scientific_name: "Myotis brandtii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 80_000.0,
    description: "Closely related to whiskered bat; prefers mature woodland. Slightly lower frequency calls. Identified reliably only by dentition or genetics.",
    echolocates: true,
};

pub const MYOTIS_MYOTIS: BatSpecies = BatSpecies {
    id: "myotis_myotis",
    name: "Greater Mouse-eared Bat",
    scientific_name: "Myotis myotis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 80_000.0,
    description: "One of Europe's largest vespertilionids. Ground-gleaning specialist hunting beetles in short grass and forest floors. Large nursery colonies in roofs and caves.",
    echolocates: true,
};

pub const MYOTIS_BECHSTEINII: BatSpecies = BatSpecies {
    id: "myotis_bechsteinii",
    name: "Bechstein's Bat",
    scientific_name: "Myotis bechsteinii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 100_000.0,
    description: "Rare woodland specialist with very quiet, broadband calls. Indicator species for old-growth forest. Roosts in tree holes; rarely found in buildings.",
    echolocates: true,
};

pub const MYOTIS_DASYCNEME: BatSpecies = BatSpecies {
    id: "myotis_dasycneme",
    name: "Pond Bat",
    scientific_name: "Myotis dasycneme",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 65_000.0,
    description: "Larger relative of Daubenton's bat. Trawls over broad lakes and slow rivers. Vulnerable across most of its range in NW Europe.",
    echolocates: true,
};

pub const NYCTALUS_NOCTULA: BatSpecies = BatSpecies {
    id: "nyctalus_noctula",
    name: "Common Noctule",
    scientific_name: "Nyctalus noctula",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 25_000.0,
    description: "Large, fast-flying bat. Loud, narrowband calls audible on bat detectors at distance. Roosts in tree holes; one of the first species to emerge at dusk.",
    echolocates: true,
};

pub const NYCTALUS_LEISLERI: BatSpecies = BatSpecies {
    id: "nyctalus_leisleri",
    name: "Leisler's Bat",
    scientific_name: "Nyctalus leisleri",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 30_000.0,
    description: "Smaller noctule with slightly higher frequency calls. Fast open-air forager. Migratory in parts of its range. Common in Ireland.",
    echolocates: true,
};

pub const NYCTALUS_LASIOPTERUS: BatSpecies = BatSpecies {
    id: "nyctalus_lasiopterus",
    name: "Greater Noctule",
    scientific_name: "Nyctalus lasiopterus",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 14_000.0,
    freq_hi_hz: 20_000.0,
    description: "Europe's largest bat. Occasionally catches small birds in flight during nocturnal migration. Very low-frequency calls. Rare across its range.",
    echolocates: true,
};

pub const EPTESICUS_SEROTINUS: BatSpecies = BatSpecies {
    id: "eptesicus_serotinus",
    name: "Serotine",
    scientific_name: "Eptesicus serotinus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 55_000.0,
    description: "Large bat with broad FM sweeps. One of the last to emerge, often foraging along tree lines and around street lights. Roosts almost exclusively in buildings.",
    echolocates: true,
};

pub const EPTESICUS_NILSSONII: BatSpecies = BatSpecies {
    id: "eptesicus_nilssonii",
    name: "Northern Bat",
    scientific_name: "Eptesicus nilssonii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 45_000.0,
    description: "The world's northernmost bat, found above the Arctic Circle. Tolerates cold climates. Common in Scandinavia and mountain regions of central Europe.",
    echolocates: true,
};

pub const PLECOTUS_AURITUS: BatSpecies = BatSpecies {
    id: "plecotus_auritus",
    name: "Brown Long-eared Bat",
    scientific_name: "Plecotus auritus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 85_000.0,
    description: "Iconic enormous ears. Very quiet, broadband calls; often called a \"whispering bat\". Gleaning specialist in woodland. Roosts in buildings and tree holes.",
    echolocates: true,
};

pub const PLECOTUS_AUSTRIACUS: BatSpecies = BatSpecies {
    id: "plecotus_austriacus",
    name: "Grey Long-eared Bat",
    scientific_name: "Plecotus austriacus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 50_000.0,
    description: "Prefers warmer lowland areas. Quieter and slightly lower frequency than brown long-eared. Difficult to distinguish visually; confirmed by tragus shape.",
    echolocates: true,
};

pub const BARBASTELLA_BARBASTELLUS: BatSpecies = BatSpecies {
    id: "barbastella_barbastellus",
    name: "Barbastelle",
    scientific_name: "Barbastella barbastellus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 45_000.0,
    description: "Distinctive flat face with upturned nose. Alternating call frequencies (~32 and ~34 kHz). Specialist moth-hunter; forest dependent. Rare across most of its range.",
    echolocates: true,
};

pub const VESPERTILIO_MURINUS: BatSpecies = BatSpecies {
    id: "vespertilio_murinus",
    name: "Parti-coloured Bat",
    scientific_name: "Vespertilio murinus",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 30_000.0,
    description: "Striking frosted fur. Alternating call frequencies. Migratory; roosts in high-rise buildings. Males produce audible courtship calls from roost entrances.",
    echolocates: true,
};

pub const RHINOLOPHUS_FERRUMEQUINUM: BatSpecies = BatSpecies {
    id: "rhinolophus_ferrumequinum",
    name: "Greater Horseshoe Bat",
    scientific_name: "Rhinolophus ferrumequinum",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 78_000.0,
    freq_hi_hz: 84_000.0,
    description: "Europe's largest horseshoe bat. Constant-frequency call at ~83 kHz. Hunts large beetles and moths in flight. Roosts in caves, mines, and old buildings.",
    echolocates: true,
};

pub const RHINOLOPHUS_HIPPOSIDEROS: BatSpecies = BatSpecies {
    id: "rhinolophus_hipposideros",
    name: "Lesser Horseshoe Bat",
    scientific_name: "Rhinolophus hipposideros",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 105_000.0,
    freq_hi_hz: 115_000.0,
    description: "One of Europe's smallest bats (~5 g). CF call at ~110 kHz. Forages close to vegetation in sheltered valleys. Very sensitive to disturbance at roost sites.",
    echolocates: true,
};

pub const RHINOLOPHUS_EURYALE: BatSpecies = BatSpecies {
    id: "rhinolophus_euryale",
    name: "Mediterranean Horseshoe Bat",
    scientific_name: "Rhinolophus euryale",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 108_000.0,
    description: "Medium-sized horseshoe bat. CF call at ~104 kHz. Restricted to Mediterranean and warm-temperate zones. Cave-dwelling; large colony roosts.",
    echolocates: true,
};

pub const TADARIDA_TENIOTIS: BatSpecies = BatSpecies {
    id: "tadarida_teniotis",
    name: "European Free-tailed Bat",
    scientific_name: "Tadarida teniotis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 18_000.0,
    description: "Europe's only free-tailed bat. Loud, low-frequency calls audible to humans. Fast, high-altitude forager. Roosts in cliff crevices and tall buildings.",
    echolocates: true,
};

pub const MINIOPTERUS_SCHREIBERSII: BatSpecies = BatSpecies {
    id: "miniopterus_schreibersii",
    name: "Common Bent-wing Bat",
    scientific_name: "Miniopterus schreibersii",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 47_000.0,
    freq_hi_hz: 57_000.0,
    description: "Fast, agile cave-dweller found across southern Europe. Long, narrow wings for sustained flight. Large colonies; sensitive to cave disturbance.",
    echolocates: true,
};

// ── Additional UK species ────────────────────────────────────────────────────

pub const MYOTIS_ALCATHOE: BatSpecies = BatSpecies {
    id: "myotis_alcathoe",
    name: "Alcathoe Bat",
    scientific_name: "Myotis alcathoe",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 105_000.0,
    description: "Tiny Myotis first described in 2001, confirmed breeding in the UK in 2010. FM calls sweep from ~100 kHz down to ~40 kHz with peak energy around 50–55 kHz. Closely resembles whiskered and Brandt's bats but smaller with shorter ears.",
    echolocates: true,
};

pub const HYPSUGO_SAVII: BatSpecies = BatSpecies {
    id: "hypsugo_savii",
    name: "Savi's Pipistrelle",
    scientific_name: "Hypsugo savii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 42_000.0,
    description: "Medium pipistrelle with shallow FM sweep ending in a quasi-constant-frequency tail around 32–34 kHz. Primarily Mediterranean; increasingly recorded as a vagrant in southern England.",
    echolocates: true,
};

// ══════════════════════════════════════════════════════════════════════════════
// Species: Costa Rica
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Jung et al. (2007): Echolocation calls in Central American emballonurids
// - Leiser-Miller & Santana (2021): Functional differences in phyllostomid echolocation (Costa Rica data)
// - Gessinger et al. (2019): Unusual CF-FM echolocation of Lonchorhina aurita
// - Schnitzler et al.: Fishing and echolocation of Noctilio leporinus
// - Kalko et al.: Echolocation and foraging of Noctilio albiventris
// - Zamora-Gutierrez et al. (2016): Acoustic identification of Mexican bats
// - Rydell et al. (2002): Acoustic identification of Yucatan bats
// - Yoh et al. (2020): Echolocation of Amazonian phyllostomid bats

// ── Noctilionidae ──────────────────────────────────────────────────────────

pub const NOCTILIO_LEPORINUS: BatSpecies = BatSpecies {
    id: "noctilio_leporinus",
    name: "Greater Bulldog Bat",
    scientific_name: "Noctilio leporinus",
    family: "Noctilionidae",
    call_type: "CF-FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 56_000.0,
    description: "Large fishing bat. Long CF component at 53\u{2013}56 kHz followed by FM sweep. Forages over water; one of few New World bats with long CF calls.",
    echolocates: true,
};

pub const NOCTILIO_ALBIVENTRIS: BatSpecies = BatSpecies {
    id: "noctilio_albiventris",
    name: "Lesser Bulldog Bat",
    scientific_name: "Noctilio albiventris",
    family: "Noctilionidae",
    call_type: "CF-FM",
    freq_lo_hz: 57_000.0,
    freq_hi_hz: 75_000.0,
    description: "Smaller than N. leporinus with higher CF at ~75 kHz. Trawls insects from water surfaces. CF-FM call structure similar to greater bulldog bat.",
    echolocates: true,
};

// ── Emballonuridae ─────────────────────────────────────────────────────────

pub const SACCOPTERYX_BILINEATA: BatSpecies = BatSpecies {
    id: "saccopteryx_bilineata",
    name: "Greater Sac-winged Bat",
    scientific_name: "Saccopteryx bilineata",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 43_000.0,
    freq_hi_hz: 49_000.0,
    description: "Alternates between ~45 and ~48 kHz when foraging. Most energy in 2nd harmonic. Well-studied species with vocal learning. Distinctive white dorsal stripes.",
    echolocates: true,
};

pub const SACCOPTERYX_LEPTURA: BatSpecies = BatSpecies {
    id: "saccopteryx_leptura",
    name: "Lesser Sac-winged Bat",
    scientific_name: "Saccopteryx leptura",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 47_000.0,
    freq_hi_hz: 53_000.0,
    description: "Higher frequency than S. bilineata. Similar multiharmonic QCF structure. Most energy in 2nd harmonic. Thinner dorsal stripes.",
    echolocates: true,
};

pub const RHYNCHONYCTERIS_NASO: BatSpecies = BatSpecies {
    id: "rhynchonycteris_naso",
    name: "Proboscis Bat",
    scientific_name: "Rhynchonycteris naso",
    family: "Emballonuridae",
    call_type: "CF-FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 100_000.0,
    description: "Tiny bat roosting along rivers. Short CF at ~47 kHz plus FM downsweep. Peak drops from ~100 kHz (search) to ~67 kHz (buzz) during prey pursuit.",
    echolocates: true,
};

pub const CORMURA_BREVIROSTRIS: BatSpecies = BatSpecies {
    id: "cormura_brevirostris",
    name: "Wagner's Sac-winged Bat",
    scientific_name: "Cormura brevirostris",
    family: "Emballonuridae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 90_000.0,
    description: "Unusual: most energy in 5th harmonic (~68 kHz). Forest-interior forager. Multiharmonic calls with shallow FM sweeps.",
    echolocates: true,
};

pub const BALANTIOPTERYX_PLICATA: BatSpecies = BatSpecies {
    id: "balantiopteryx_plicata",
    name: "Gray Sac-winged Bat",
    scientific_name: "Balantiopteryx plicata",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 50_000.0,
    description: "Open-area forager. Long narrowband QCF (14\u{2013}20 ms) plus short broadband component. Displays jamming avoidance by shifting peak frequency in groups.",
    echolocates: true,
};

pub const PEROPTERYX_MACROTIS: BatSpecies = BatSpecies {
    id: "peropteryx_macrotis",
    name: "Lesser Dog-like Bat",
    scientific_name: "Peropteryx macrotis",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 50_000.0,
    description: "Multiharmonic QCF with most energy in 2nd harmonic at ~40 kHz. Higher frequency than P. kappleri.",
    echolocates: true,
};

pub const PEROPTERYX_KAPPLERI: BatSpecies = BatSpecies {
    id: "peropteryx_kappleri",
    name: "Greater Dog-like Bat",
    scientific_name: "Peropteryx kappleri",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 40_000.0,
    description: "Lower frequency (~32 kHz) than P. macrotis, consistent with larger body size. Open-space forager. Most energy in 2nd harmonic.",
    echolocates: true,
};

pub const DICLIDURUS_ALBUS: BatSpecies = BatSpecies {
    id: "diclidurus_albus",
    name: "Northern Ghost Bat",
    scientific_name: "Diclidurus albus",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 27_000.0,
    description: "Large emballonurid with distinctive white fur. Narrowband QCF at ~24 kHz. Long inter-pulse intervals. Rarely encountered.",
    echolocates: true,
};

// ── Mormoopidae ────────────────────────────────────────────────────────────

pub const PTERONOTUS_MESOAMERICANUS: BatSpecies = BatSpecies {
    id: "pteronotus_mesoamericanus",
    name: "Mesoamerican Mustached Bat",
    scientific_name: "Pteronotus mesoamericanus",
    family: "Mormoopidae",
    call_type: "CF-FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 120_000.0,
    description: "The ONLY New World high-duty-cycle echolocator. Long CF at ~61 kHz (2nd harmonic) with Doppler compensation. 4\u{2013}5 harmonics. Formerly P. parnellii.",
    echolocates: true,
};

pub const PTERONOTUS_DAVYI: BatSpecies = BatSpecies {
    id: "pteronotus_davyi",
    name: "Davy's Naked-backed Bat",
    scientific_name: "Pteronotus davyi",
    family: "Mormoopidae",
    call_type: "CF-FM",
    freq_lo_hz: 51_000.0,
    freq_hi_hz: 68_000.0,
    description: "CF-FM with short CF at ~67 kHz, FM sweep down to ~51 kHz. Shorter calls than P. mesoamericanus. Most energy in 2nd harmonic. Wing membranes fused across back.",
    echolocates: true,
};

pub const PTERONOTUS_GYMNONOTUS: BatSpecies = BatSpecies {
    id: "pteronotus_gymnonotus",
    name: "Big Naked-backed Bat",
    scientific_name: "Pteronotus gymnonotus",
    family: "Mormoopidae",
    call_type: "CF-FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 65_000.0,
    description: "CF component at ~54\u{2013}57 kHz. Similar structure to P. davyi but lower frequency, consistent with larger body size.",
    echolocates: true,
};

pub const PTERONOTUS_PERSONATUS: BatSpecies = BatSpecies {
    id: "pteronotus_personatus",
    name: "Wagner's Mustached Bat",
    scientific_name: "Pteronotus personatus",
    family: "Mormoopidae",
    call_type: "CF-FM",
    freq_lo_hz: 62_000.0,
    freq_hi_hz: 83_000.0,
    description: "Highest frequency Pteronotus in Costa Rica (excl. P. mesoamericanus harmonics). Initial CF ~83 kHz, terminal CF ~68 kHz. Shows Doppler-shift compensation.",
    echolocates: true,
};

pub const MORMOOPS_MEGALOPHYLLA: BatSpecies = BatSpecies {
    id: "mormoops_megalophylla",
    name: "Ghost-faced Bat",
    scientific_name: "Mormoops megalophylla",
    family: "Mormoopidae",
    call_type: "QCF",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 70_000.0,
    description: "Bizarre facial leaf. Fundamental ~30 kHz suppressed; only 2nd harmonic (~67 kHz) typically recorded. Cave-roosting; large colonies.",
    echolocates: true,
};

// ── Phyllostomidae: Phyllostominae (insectivores/carnivores) ───────────────

pub const VAMPYRUM_SPECTRUM: BatSpecies = BatSpecies {
    id: "vampyrum_spectrum",
    name: "Spectral Bat",
    scientific_name: "Vampyrum spectrum",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 90_000.0,
    description: "Largest bat in the Americas (wingspan ~1 m). Low-intensity multiharmonic FM. Carnivorous: preys on birds and other bats. Very difficult to detect acoustically.",
    echolocates: true,
};

pub const CHROTOPTERUS_AURITUS: BatSpecies = BatSpecies {
    id: "chrotopterus_auritus",
    name: "Big-eared Woolly Bat",
    scientific_name: "Chrotopterus auritus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 67_000.0,
    freq_hi_hz: 88_000.0,
    description: "Carnivorous gleaner. Peak ~77 kHz, higher than predicted for body size. Short FM sweeps (0.8\u{2013}1.4 ms). Low-intensity; hard to detect beyond a few meters.",
    echolocates: true,
};

pub const PHYLLOSTOMUS_HASTATUS: BatSpecies = BatSpecies {
    id: "phyllostomus_hastatus",
    name: "Greater Spear-nosed Bat",
    scientific_name: "Phyllostomus hastatus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 34_000.0,
    freq_hi_hz: 60_000.0,
    description: "Large omnivore. One of the lowest-frequency phyllostomids (~47 kHz peak). Multiharmonic FM. Forms harem groups in caves and hollow trees.",
    echolocates: true,
};

pub const PHYLLOSTOMUS_DISCOLOR: BatSpecies = BatSpecies {
    id: "phyllostomus_discolor",
    name: "Pale Spear-nosed Bat",
    scientific_name: "Phyllostomus discolor",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 80_000.0,
    description: "Omnivore with lower frequency than most phyllostomids (~55 kHz peak). Best hearing sensitivity at 20 kHz. Low-intensity whispering calls.",
    echolocates: true,
};

pub const LONCHORHINA_AURITA: BatSpecies = BatSpecies {
    id: "lonchorhina_aurita",
    name: "Tomes's Sword-nosed Bat",
    scientific_name: "Lonchorhina aurita",
    family: "Phyllostomidae",
    call_type: "CF-FM",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 50_000.0,
    description: "UNIQUE among phyllostomids: uses CF-FM calls like mormoopids. Long CF at ~45 kHz (3rd harmonic dominant). Longest calls of any phyllostomid (up to 8.7 ms).",
    echolocates: true,
};

pub const TRACHOPS_CIRRHOSUS: BatSpecies = BatSpecies {
    id: "trachops_cirrhosus",
    name: "Fringe-lipped Bat",
    scientific_name: "Trachops cirrhosus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 100_000.0,
    description: "Famous frog-eating bat. Listens for frog mating calls to locate prey. Low-intensity multiharmonic FM (~70 kHz peak). Hard to detect acoustically.",
    echolocates: true,
};

pub const MICRONYCTERIS_MICROTIS: BatSpecies = BatSpecies {
    id: "micronycteris_microtis",
    name: "Common Big-eared Bat",
    scientific_name: "Micronycteris microtis",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 120_000.0,
    description: "Gleaning insectivore in dense understory. Can detect silent, motionless prey. Very short broadband calls (0.3\u{2013}1 ms) at high frequency (~90\u{2013}100 kHz). Ultra-low intensity.",
    echolocates: true,
};

pub const MICRONYCTERIS_HIRSUTA: BatSpecies = BatSpecies {
    id: "micronycteris_hirsuta",
    name: "Hairy Big-eared Bat",
    scientific_name: "Micronycteris hirsuta",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 51_000.0,
    freq_hi_hz: 90_000.0,
    description: "Lower peak frequency (~52 kHz) than M. microtis. Gleaning insectivore. Low-intensity multiharmonic FM. Documented from Costa Rica.",
    echolocates: true,
};

pub const LOPHOSTOMA_SILVICOLUM: BatSpecies = BatSpecies {
    id: "lophostoma_silvicolum",
    name: "White-throated Round-eared Bat",
    scientific_name: "Lophostoma silvicolum",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 100_000.0,
    description: "Gleaning insectivore. Modifies termite nests to create roosts. Peak ~70 kHz. Low-intensity multiharmonic FM. Difficult to detect beyond a few meters.",
    echolocates: true,
};

pub const MIMON_CRENULATUM: BatSpecies = BatSpecies {
    id: "mimon_crenulatum",
    name: "Striped Hairy-nosed Bat",
    scientific_name: "Gardnerycteris crenulatum",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 100_000.0,
    description: "Gleaning insectivore. Peak ~75 kHz. Low-intensity multiharmonic FM. Now placed in genus Gardnerycteris by some authorities.",
    echolocates: true,
};

pub const TONATIA_SAUROPHILA: BatSpecies = BatSpecies {
    id: "tonatia_saurophila",
    name: "Stripe-headed Round-eared Bat",
    scientific_name: "Tonatia saurophila",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 90_000.0,
    description: "Gleaning insectivore/carnivore. Peak ~65 kHz. Low-intensity multiharmonic FM. Forest interior species.",
    echolocates: true,
};

pub const MACROPHYLLUM_MACROPHYLLUM: BatSpecies = BatSpecies {
    id: "macrophyllum_macrophyllum",
    name: "Long-legged Bat",
    scientific_name: "Macrophyllum macrophyllum",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 110_000.0,
    description: "Unusual phyllostomid: trawls insects from water surfaces. Louder than most phyllostomids (~101 dB SPL). Adjusts intensity by habitat openness. Peak ~85 kHz.",
    echolocates: true,
};

pub const LAMPRONYCTERIS_BRACHYOTIS: BatSpecies = BatSpecies {
    id: "lampronycteris_brachyotis",
    name: "Yellow-throated Big-eared Bat",
    scientific_name: "Lampronycteris brachyotis",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 100_000.0,
    description: "Rare gleaning insectivore. Peak ~75 kHz. Low-intensity multiharmonic FM. Poorly documented acoustically.",
    echolocates: true,
};

pub const GLYPHONYCTERIS_SYLVESTRIS: BatSpecies = BatSpecies {
    id: "glyphonycteris_sylvestris",
    name: "Tricolored Big-eared Bat",
    scientific_name: "Glyphonycteris sylvestris",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 110_000.0,
    description: "Rare gleaning insectivore. Peak ~85 kHz. Very short broadband FM calls (0.3\u{2013}1 ms). Ultra-low intensity.",
    echolocates: true,
};

pub const TRINYCTERIS_NICEFORI: BatSpecies = BatSpecies {
    id: "trinycteris_nicefori",
    name: "Niceforo's Big-eared Bat",
    scientific_name: "Trinycteris nicefori",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 100_000.0,
    description: "Low-intensity gleaner. Peak ~80 kHz. Multiharmonic FM. Forest understory specialist.",
    echolocates: true,
};

// ── Phyllostomidae: Glossophaginae (nectar bats) ───────────────────────────

pub const GLOSSOPHAGA_SORICINA: BatSpecies = BatSpecies {
    id: "glossophaga_soricina",
    name: "Pallas's Long-tongued Bat",
    scientific_name: "Glossophaga soricina",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 140_000.0,
    description: "Nectarivore that uses echolocation to find flowers (some have evolved acoustic reflectors). Peak ~80 kHz. Multiharmonic FM. Low intensity.",
    echolocates: true,
};

pub const GLOSSOPHAGA_COMMISSARISI: BatSpecies = BatSpecies {
    id: "glossophaga_commissarisi",
    name: "Commissaris's Long-tongued Bat",
    scientific_name: "Glossophaga commissarisi",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 130_000.0,
    description: "Similar call structure to G. soricina but slightly lower peak (~75 kHz). Nectarivore. Low-intensity multiharmonic FM.",
    echolocates: true,
};

pub const ANOURA_GEOFFROYI: BatSpecies = BatSpecies {
    id: "anoura_geoffroyi",
    name: "Geoffroy's Tailless Bat",
    scientific_name: "Anoura geoffroyi",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 120_000.0,
    description: "High-altitude nectarivore. Peak ~70 kHz. Low-intensity multiharmonic FM. Found in cloud forests and highlands.",
    echolocates: true,
};

pub const HYLONYCTERIS_UNDERWOODI: BatSpecies = BatSpecies {
    id: "hylonycteris_underwoodi",
    name: "Underwood's Long-tongued Bat",
    scientific_name: "Hylonycteris underwoodi",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 130_000.0,
    description: "Very small nectarivore. High frequency calls (~90 kHz peak). Low-intensity FM. Found in montane forests.",
    echolocates: true,
};

// ── Phyllostomidae: Carolliinae (short-tailed bats) ────────────────────────

pub const CAROLLIA_PERSPICILLATA: BatSpecies = BatSpecies {
    id: "carollia_perspicillata",
    name: "Seba's Short-tailed Bat",
    scientific_name: "Carollia perspicillata",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 100_000.0,
    description: "One of the most abundant Neotropical bats. Peak ~71 kHz. Frugivore that also takes insects. Low-intensity whispering calls.",
    echolocates: true,
};

pub const CAROLLIA_CASTANEA: BatSpecies = BatSpecies {
    id: "carollia_castanea",
    name: "Chestnut Short-tailed Bat",
    scientific_name: "Carollia castanea",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 110_000.0,
    description: "Higher peak (~78 kHz) than C. perspicillata, consistent with smaller body. Frugivore. Low-intensity FM. Documented from Costa Rica.",
    echolocates: true,
};

pub const CAROLLIA_BREVICAUDA: BatSpecies = BatSpecies {
    id: "carollia_brevicauda",
    name: "Silky Short-tailed Bat",
    scientific_name: "Carollia brevicauda",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 100_000.0,
    description: "Intermediate in size and frequency (~73 kHz peak) between C. perspicillata and C. castanea. Frugivore. Low-intensity FM.",
    echolocates: true,
};

// ── Phyllostomidae: Desmodontinae (vampire bats) ──────────────────────────

pub const DESMODUS_ROTUNDUS: BatSpecies = BatSpecies {
    id: "desmodus_rotundus",
    name: "Common Vampire Bat",
    scientific_name: "Desmodus rotundus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 85_000.0,
    description: "Relatively long calls for a phyllostomid (~5.5 ms). Peak ~55 kHz. Obligate blood-feeder. Emits single pulses or groups of 2\u{2013}3.",
    echolocates: true,
};

// ── Phyllostomidae: Stenodermatinae (fruit bats) ──────────────────────────

pub const ARTIBEUS_JAMAICENSIS: BatSpecies = BatSpecies {
    id: "artibeus_jamaicensis",
    name: "Jamaican Fruit Bat",
    scientific_name: "Artibeus jamaicensis",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 90_000.0,
    description: "Very common frugivore. Peak ~56 kHz. Variable intensity (mean ~96 dB SPL); not always a quiet \"whisperer\". Important seed disperser.",
    echolocates: true,
};

pub const ARTIBEUS_LITURATUS: BatSpecies = BatSpecies {
    id: "artibeus_lituratus",
    name: "Great Fruit-eating Bat",
    scientific_name: "Artibeus lituratus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 85_000.0,
    description: "Large frugivore. Lower peak (~52 kHz) than A. jamaicensis, consistent with larger body. Prominent facial stripes. Important pollinator.",
    echolocates: true,
};

pub const STURNIRA_LILIUM: BatSpecies = BatSpecies {
    id: "sturnira_lilium",
    name: "Little Yellow-shouldered Bat",
    scientific_name: "Sturnira lilium",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 76_000.0,
    description: "Frugivore. Peak ~66.5 kHz (well documented). Low-intensity multiharmonic FM. Common in forest and edge habitats.",
    echolocates: true,
};

pub const URODERMA_BILOBATUM: BatSpecies = BatSpecies {
    id: "uroderma_bilobatum",
    name: "Tent-making Bat",
    scientific_name: "Uroderma bilobatum",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 95_000.0,
    description: "Tent-roosting frugivore. Modifies large leaves into tents. Peak ~70 kHz. Low-intensity nasal FM. Found in lowland forests.",
    echolocates: true,
};

pub const ECTOPHYLLA_ALBA: BatSpecies = BatSpecies {
    id: "ectophylla_alba",
    name: "Honduran White Bat",
    scientific_name: "Ectophylla alba",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 95_000.0,
    description: "Tiny, iconic white tent-roosting bat. Peak ~75 kHz. Low-intensity FM. Endemic to Central America (Honduras to Panama). Frugivore specializing on one fig species.",
    echolocates: true,
};

pub const CENTURIO_SENEX: BatSpecies = BatSpecies {
    id: "centurio_senex",
    name: "Wrinkle-faced Bat",
    scientific_name: "Centurio senex",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 90_000.0,
    description: "Bizarre wrinkled face with skin fold that can cover it like a mask. Peak ~65 kHz. Relatively long calls for a stenodermatine (1\u{2013}3 ms). Frugivore.",
    echolocates: true,
};

pub const DERMANURA_PHAEOTIS: BatSpecies = BatSpecies {
    id: "dermanura_phaeotis",
    name: "Pygmy Fruit-eating Bat",
    scientific_name: "Dermanura phaeotis",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 100_000.0,
    description: "Small frugivore (formerly Artibeus phaeotis). Peak ~75 kHz. Low-intensity multiharmonic FM. Common in lowland forests.",
    echolocates: true,
};

pub const MESOPHYLLA_MACCONNELLI: BatSpecies = BatSpecies {
    id: "mesophylla_macconnelli",
    name: "MacConnell's Bat",
    scientific_name: "Mesophylla macconnelli",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 70_000.0,
    freq_hi_hz: 118_000.0,
    description: "Very small (5\u{2013}7 g) with the highest peak frequency recorded among phyllostomids (~100\u{2013}118 kHz). Ultra-low intensity. Tent-roosting frugivore.",
    echolocates: true,
};

// ── Molossidae ─────────────────────────────────────────────────────────────

pub const MOLOSSUS_MOLOSSUS: BatSpecies = BatSpecies {
    id: "molossus_molossus",
    name: "Velvety Free-tailed Bat",
    scientific_name: "Molossus molossus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 33_000.0,
    freq_hi_hz: 50_000.0,
    description: "Alternates between ~34.5 and ~39.6 kHz when foraging. Narrowband QCF. Common in buildings and urban areas. Open-space aerial hawker.",
    echolocates: true,
};

pub const MOLOSSUS_SINALOAE: BatSpecies = BatSpecies {
    id: "molossus_sinaloae",
    name: "Sinaloan Mastiff Bat",
    scientific_name: "Molossus sinaloae",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 31_000.0,
    freq_hi_hz: 57_000.0,
    description: "QCF at ~34 kHz in natural habitat; shifts up ~6 kHz in urban noise (Lombard effect). Open-space forager. Larger than M. molossus.",
    echolocates: true,
};

pub const MOLOSSUS_RUFUS: BatSpecies = BatSpecies {
    id: "molossus_rufus",
    name: "Black Mastiff Bat",
    scientific_name: "Molossus rufus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 24_000.0,
    freq_hi_hz: 28_000.0,
    description: "Large molossid with low frequency (~25\u{2013}26 kHz). Open-space forager. Formerly M. ater. Roosts in buildings and hollow trees.",
    echolocates: true,
};

pub const MOLOSSUS_BONDAE: BatSpecies = BatSpecies {
    id: "molossus_bondae",
    name: "Bonda Mastiff Bat",
    scientific_name: "Molossus bondae",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 42_000.0,
    description: "Similar to M. molossus but slightly lower frequency (~33 kHz). Open-space forager. Roosts in buildings.",
    echolocates: true,
};

pub const MOLOSSUS_COIBENSIS: BatSpecies = BatSpecies {
    id: "molossus_coibensis",
    name: "Coiban Mastiff Bat",
    scientific_name: "Molossus coibensis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 42_000.0,
    description: "Similar to M. molossus (~35 kHz). Originally described from Coiba Island, Panama. Open-space forager.",
    echolocates: true,
};

pub const EUMOPS_AURIPENDULUS: BatSpecies = BatSpecies {
    id: "eumops_auripendulus",
    name: "Black Bonneted Bat",
    scientific_name: "Eumops auripendulus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 36_000.0,
    description: "Large molossid. Alternating QCF calls at ~23\u{2013}26 kHz. High, fast flight. Long-duration narrowband calls.",
    echolocates: true,
};

pub const EUMOPS_GLAUCINUS: BatSpecies = BatSpecies {
    id: "eumops_glaucinus",
    name: "Wagner's Bonneted Bat",
    scientific_name: "Eumops glaucinus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 19_000.0,
    freq_hi_hz: 29_000.0,
    description: "Among the lowest-frequency calling molossids (~22\u{2013}25 kHz). Large bat with long, narrow wings. High-altitude open-space forager.",
    echolocates: true,
};

pub const CYNOMOPS_GREENHALLI: BatSpecies = BatSpecies {
    id: "cynomops_greenhalli",
    name: "Greenhall's Dog-faced Bat",
    scientific_name: "Cynomops greenhalli",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 17_000.0,
    freq_hi_hz: 29_000.0,
    description: "Low frequency (~22 kHz) open-space forager. Flat-faced with forward-pointing nostrils. Roosts in buildings and hollow trees.",
    echolocates: true,
};

pub const TADARIDA_BRASILIENSIS: BatSpecies = BatSpecies {
    id: "tadarida_brasiliensis",
    name: "Mexican Free-tailed Bat",
    scientific_name: "Tadarida brasiliensis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 75_000.0,
    description: "Extremely flexible acoustics: narrow QCF 49\u{2013}70 kHz in open space, drops to 25\u{2013}40 kHz near objects, broadband FM during approach. Forms massive colonies.",
    echolocates: true,
};

pub const PROMOPS_CENTRALIS: BatSpecies = BatSpecies {
    id: "promops_centralis",
    name: "Big-crested Mastiff Bat",
    scientific_name: "Promops centralis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 35_000.0,
    description: "Distinctive upward-modulated calls (unusual for molossids). Alternating pulse pairs at ~30 and ~35 kHz. Most energy in 1st harmonic. Easily recognizable.",
    echolocates: true,
};

pub const NYCTINOMOPS_LATICAUDATUS: BatSpecies = BatSpecies {
    id: "nyctinomops_laticaudatus",
    name: "Broad-eared Free-tailed Bat",
    scientific_name: "Nyctinomops laticaudatus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 24_000.0,
    freq_hi_hz: 32_000.0,
    description: "Three-frequency alternation pattern (~26.7, 28.7, 32.4 kHz). Downward-modulated QCF. Open-space forager.",
    echolocates: true,
};

// ── Vespertilionidae ───────────────────────────────────────────────────────

pub const MYOTIS_NIGRICANS: BatSpecies = BatSpecies {
    id: "myotis_nigricans",
    name: "Black Myotis",
    scientific_name: "Myotis nigricans",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 80_000.0,
    description: "Highly plastic call structure. Narrowband ~7 ms in open space; adds broadband FM in clutter. Convergent with European pipistrelles. Peak ~50 kHz.",
    echolocates: true,
};

pub const MYOTIS_KEAYSI: BatSpecies = BatSpecies {
    id: "myotis_keaysi",
    name: "Hairy-legged Myotis",
    scientific_name: "Myotis keaysi",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 100_000.0,
    description: "High variable repetition rates (15\u{2013}20 calls/s). Short, high-intensity calls (~2.5 ms). Peak ~55 kHz. Recorded from colonies in Costa Rica.",
    echolocates: true,
};

pub const MYOTIS_RIPARIUS: BatSpecies = BatSpecies {
    id: "myotis_riparius",
    name: "Riparian Myotis",
    scientific_name: "Myotis riparius",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 120_000.0,
    description: "Steep broadband FM sweep from ~120 kHz to ~50 kHz. Short calls (~2 ms). No harmonics observed. Recorded in Costa Rica.",
    echolocates: true,
};

pub const MYOTIS_ELEGANS: BatSpecies = BatSpecies {
    id: "myotis_elegans",
    name: "Elegant Myotis",
    scientific_name: "Myotis elegans",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 100_000.0,
    description: "High-frequency calls typical of small Myotis. Peak ~55 kHz. Difficult to distinguish from M. nigricans acoustically.",
    echolocates: true,
};

pub const EPTESICUS_BRASILIENSIS: BatSpecies = BatSpecies {
    id: "eptesicus_brasiliensis",
    name: "Brazilian Brown Bat",
    scientific_name: "Eptesicus brasiliensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 70_000.0,
    description: "Peak ~54\u{2013}60 kHz. Source level ~101\u{2013}106 dB SPL. Frequency decreases with temperature. FM-QCF structure.",
    echolocates: true,
};

pub const EPTESICUS_FURINALIS: BatSpecies = BatSpecies {
    id: "eptesicus_furinalis",
    name: "Argentine Brown Bat",
    scientific_name: "Eptesicus furinalis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 45_000.0,
    description: "Lower frequency than E. brasiliensis (~43 kHz peak). More FM in cluttered habitats. FM-QCF structure.",
    echolocates: true,
};

pub const EPTESICUS_FUSCUS: BatSpecies = BatSpecies {
    id: "eptesicus_fuscus",
    name: "Big Brown Bat",
    scientific_name: "Eptesicus fuscus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 50_000.0,
    description: "One of the best-studied bat species globally. Large vespertilionid. Peak ~30 kHz. FM-QCF. Open-space forager. Common in buildings.",
    echolocates: true,
};

pub const LASIURUS_BLOSSEVILLII: BatSpecies = BatSpecies {
    id: "lasiurus_blossevillii",
    name: "Desert Red Bat",
    scientific_name: "Lasiurus blossevillii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 23_000.0,
    freq_hi_hz: 52_000.0,
    description: "Open-air forager. Maximum energy ~42 kHz. FM-QCF. Migratory. Roosts solitarily in foliage. Distinctive reddish fur.",
    echolocates: true,
};

pub const LASIURUS_EGA: BatSpecies = BatSpecies {
    id: "lasiurus_ega",
    name: "Southern Yellow Bat",
    scientific_name: "Lasiurus ega",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 50_000.0,
    description: "Lower peak (~35 kHz) than L. blossevillii. FM-QCF. Open-air forager. Roosts in palm fronds.",
    echolocates: true,
};

pub const RHOGEESSA_TUMIDA: BatSpecies = BatSpecies {
    id: "rhogeessa_tumida",
    name: "Black-winged Little Yellow Bat",
    scientific_name: "Rhogeessa tumida",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 43_000.0,
    freq_hi_hz: 55_000.0,
    description: "Small vespertilionid. Broadband steep FM with small QCF termination. Peak ~48 kHz. Bandwidth >15 kHz.",
    echolocates: true,
};

pub const BAUERUS_DUBIAQUERCUS: BatSpecies = BatSpecies {
    id: "bauerus_dubiaquercus",
    name: "Van Gelder's Bat",
    scientific_name: "Bauerus dubiaquercus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 55_000.0,
    description: "Very quiet calls. Related to long-eared bats (Plecotus-like). Gleaning insectivore. Peak ~35 kHz. Rare, poorly known.",
    echolocates: true,
};

// ── Thyropteridae ──────────────────────────────────────────────────────────

pub const THYROPTERA_TRICOLOR: BatSpecies = BatSpecies {
    id: "thyroptera_tricolor",
    name: "Spix's Disk-winged Bat",
    scientific_name: "Thyroptera tricolor",
    family: "Thyropteridae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 70_000.0,
    description: "Suction-cup disks on wrists and ankles for roosting in rolled Heliconia leaves. Extremely low intensity; barely detectable at <1 m. Distinctive social calls for roost-finding.",
    echolocates: true,
};

// ── Natalidae ──────────────────────────────────────────────────────────────

pub const NATALUS_MEXICANUS: BatSpecies = BatSpecies {
    id: "natalus_mexicanus",
    name: "Mexican Funnel-eared Bat",
    scientific_name: "Natalus mexicanus",
    family: "Natalidae",
    call_type: "FM",
    freq_lo_hz: 85_000.0,
    freq_hi_hz: 170_000.0,
    description: "Among the highest frequency echolocating bats. Peak ~100\u{2013}130 kHz (2nd harmonic). Very low intensity; barely detectable beyond 50 cm. Cave-roosting.",
    echolocates: true,
};

// ══════════════════════════════════════════════════════════════════════════════
// Japan species
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Fukui et al. (2004) Zool Sci: Acoustic ID of eight Hokkaido species
// - Funakoshi & Uchida (1978): R. ferrumequinum nippon CF frequency
// - Hiryu et al. (2006): R. pumilus geographic variation on Okinawa
// - Fujioka et al. (2014): CNN bat species ID system for 30 Japanese species
// - IUCN Red List; Ministry of the Environment (Japan) Red Data Book

// ── Pteropodidae ─────────────────────────────────────────────────────────────

pub const PTEROPUS_DASYMALLUS: BatSpecies = BatSpecies {
    id: "pteropus_dasymallus",
    name: "Ryukyu Flying Fox",
    scientific_name: "Pteropus dasymallus",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Large fruit bat of the Ryukyu Islands (4 subspecies in Japan). Does not echolocate. Feeds on fruit, flowers, and leaves. Endangered due to habitat loss and hunting.",
    echolocates: false,
};

// ── Rhinolophidae ────────────────────────────────────────────────────────────

pub const RHINOLOPHUS_FERRUMEQUINUM_NIPPON: BatSpecies = BatSpecies {
    id: "rhinolophus_ferrumequinum_nippon",
    name: "Japanese Greater Horseshoe Bat",
    scientific_name: "Rhinolophus ferrumequinum nippon",
    family: "Rhinolophidae",
    call_type: "CF-FM",
    freq_lo_hz: 58_000.0,
    freq_hi_hz: 72_000.0,
    description: "CF-FM calls with CF2 at ~65\u{2013}69 kHz. FM/CF/FM structure. Widespread from Hokkaido to Kyushu. Roosts in caves, mines, and tunnels.",
    echolocates: true,
};

pub const RHINOLOPHUS_CORNUTUS: BatSpecies = BatSpecies {
    id: "rhinolophus_cornutus",
    name: "Japanese Lesser Horseshoe Bat",
    scientific_name: "Rhinolophus cornutus",
    family: "Rhinolophidae",
    call_type: "CF-FM",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 120_000.0,
    description: "CF-FM calls with peak CF ~103\u{2013}111 kHz, increasing from north to south. Endemic to Japan (Honshu, Shikoku, Kyushu). Roosts in caves and buildings.",
    echolocates: true,
};

pub const RHINOLOPHUS_PUMILUS: BatSpecies = BatSpecies {
    id: "rhinolophus_pumilus",
    name: "Okinawa Least Horseshoe Bat",
    scientific_name: "Rhinolophus pumilus",
    family: "Rhinolophidae",
    call_type: "CF-FM",
    freq_lo_hz: 80_000.0,
    freq_hi_hz: 92_000.0,
    description: "CF-FM calls with CF ~80\u{2013}90 kHz. Shows 5\u{2013}8 kHz dialect difference between northern and southern Okinawa populations. Endemic to the central and southern Ryukyus.",
    echolocates: true,
};

pub const RHINOLOPHUS_PERDITUS: BatSpecies = BatSpecies {
    id: "rhinolophus_perditus",
    name: "Yaeyama Horseshoe Bat",
    scientific_name: "Rhinolophus perditus",
    family: "Rhinolophidae",
    call_type: "CF-FM",
    freq_lo_hz: 90_000.0,
    freq_hi_hz: 100_000.0,
    description: "CF-FM calls with peak CF ~92\u{2013}98 kHz (92\u{2013}93 on Iriomote, 96\u{2013}98 on Ishigaki). Endemic to the Yaeyama Islands. Forest-dwelling, cave-roosting.",
    echolocates: true,
};

pub const RHINOLOPHUS_IMAIZUMII: BatSpecies = BatSpecies {
    id: "rhinolophus_imaizumii",
    name: "Imaizumi's Horseshoe Bat",
    scientific_name: "Rhinolophus imaizumii",
    family: "Rhinolophidae",
    call_type: "CF-FM",
    freq_lo_hz: 94_000.0,
    freq_hi_hz: 108_000.0,
    description: "CF-FM calls at frequency intermediate between R. cornutus and R. pumilus. Temperate forests on Honshu and Shikoku. Taxonomic status debated (sometimes synonymised with R. pusillus).",
    echolocates: true,
};

// ── Hipposideridae ───────────────────────────────────────────────────────────

pub const HIPPOSIDEROS_TURPIS: BatSpecies = BatSpecies {
    id: "hipposideros_turpis",
    name: "Temminck's Leaf-nosed Bat",
    scientific_name: "Hipposideros turpis",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 65_000.0,
    freq_hi_hz: 80_000.0,
    description: "CF calls typical of hipposiderids. Southern Ryukyu Islands (Ishigaki, Iriomote, Miyako). Roosts in limestone caves. Feeds mainly on beetles.",
    echolocates: true,
};

// ── Emballonuridae ───────────────────────────────────────────────────────────

pub const TAPHOZOUS_MELANOPOGON: BatSpecies = BatSpecies {
    id: "taphozous_melanopogon",
    name: "Black-bearded Tomb Bat",
    scientific_name: "Taphozous melanopogon",
    family: "Emballonuridae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 40_000.0,
    description: "Low-intensity FM calls, dominant frequency ~29\u{2013}33 kHz with four harmonics and most energy in the second. Ryukyu Islands. Roosts on rock walls and in caves.",
    echolocates: true,
};

// ── Miniopteridae ────────────────────────────────────────────────────────────

pub const MINIOPTERUS_FULIGINOSUS: BatSpecies = BatSpecies {
    id: "miniopterus_fuliginosus",
    name: "Eastern Bent-wing Bat",
    scientific_name: "Miniopterus fuliginosus",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 44_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM downward sweeps, terminal frequency ~44\u{2013}50 kHz. Fast agile flier. Cave-roosting, forms large maternity colonies. Widespread from Honshu to the Ryukyus.",
    echolocates: true,
};

pub const MINIOPTERUS_FUSCUS: BatSpecies = BatSpecies {
    id: "miniopterus_fuscus",
    name: "Ryukyu Bent-wing Bat",
    scientific_name: "Miniopterus fuscus",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 105_000.0,
    description: "FM calls slightly higher frequency than M. fuliginosus due to smaller body size. Ryukyu Islands and southern Kyushu. Cave-roosting.",
    echolocates: true,
};

// ── Molossidae ───────────────────────────────────────────────────────────────

pub const TADARIDA_INSIGNIS: BatSpecies = BatSpecies {
    id: "tadarida_insignis",
    name: "East Asian Free-tailed Bat",
    scientific_name: "Tadarida insignis",
    family: "Molossidae",
    call_type: "FM-QCF",
    freq_lo_hz: 15_000.0,
    freq_hi_hz: 30_000.0,
    description: "Low-frequency FM-QCF calls ~18\u{2013}25 kHz. Fast, high-flying open-air forager. Western Japan. Roosts in rock crevices and buildings.",
    echolocates: true,
};

pub const TADARIDA_LATOUCHEI: BatSpecies = BatSpecies {
    id: "tadarida_latouchei",
    name: "La Touche's Free-tailed Bat",
    scientific_name: "Tadarida latouchei",
    family: "Molossidae",
    call_type: "FM-QCF",
    freq_lo_hz: 15_000.0,
    freq_hi_hz: 25_000.0,
    description: "Echolocation ~20 kHz. High-altitude flier, very difficult to capture. Known in Japan from a single specimen on Amami-Oshima (1985). IUCN Data Deficient.",
    echolocates: true,
};

// ── Vespertilionidae: Myotis ─────────────────────────────────────────────────

pub const MYOTIS_MACRODACTYLUS: BatSpecies = BatSpecies {
    id: "myotis_macrodactylus",
    name: "Large-footed Bat",
    scientific_name: "Myotis macrodactylus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 90_000.0,
    description: "Steep FM sweeps ~90\u{2013}40 kHz, peak ~50 kHz. Trawling insectivore, hunts over water using large feet to gaff prey. Rivers and streams throughout Japan.",
    echolocates: true,
};

pub const MYOTIS_IKONNIKOVI: BatSpecies = BatSpecies {
    id: "myotis_ikonnikovi",
    name: "Ikonnikov's Bat",
    scientific_name: "Myotis ikonnikovi",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 37_000.0,
    freq_hi_hz: 120_000.0,
    description: "Steep FM sweeps, peak ~50.6 kHz, start ~90 kHz, end ~43 kHz, duration ~2 ms. Small forest bat. Hokkaido and northern Honshu.",
    echolocates: true,
};

pub const MYOTIS_BOMBINUS: BatSpecies = BatSpecies {
    id: "myotis_bombinus",
    name: "Far Eastern Natterer's Bat",
    scientific_name: "Myotis bombinus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 150_000.0,
    description: "Very broadband FM calls sweeping from ~150 kHz down to ~20 kHz. Gleaning insectivore, hawks near vegetation. Forests of Kyushu and other regions.",
    echolocates: true,
};

pub const MYOTIS_PRUINOSUS: BatSpecies = BatSpecies {
    id: "myotis_pruinosus",
    name: "Frosted Myotis",
    scientific_name: "Myotis pruinosus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM sweeps typical of Myotis. Endemic to Japan (Honshu, Shikoku, Kyushu). Forest-dwelling, roosts in tree hollows and buildings. Named for frosted fur.",
    echolocates: true,
};

pub const MYOTIS_YANBARENSIS: BatSpecies = BatSpecies {
    id: "myotis_yanbarensis",
    name: "Yanbaru Whiskered Bat",
    scientific_name: "Myotis yanbarensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM calls above 40 kHz. Endemic to northern Okinawa (Yanbaru forest). Described in 1997. One of the rarest bats in Japan.",
    echolocates: true,
};

pub const MYOTIS_FRATER: BatSpecies = BatSpecies {
    id: "myotis_frater",
    name: "Fraternal Bat",
    scientific_name: "Myotis frater",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 110_000.0,
    description: "Brief FM pulses ~110\u{2013}50 kHz, duration ~3.5 ms. Edge-space forager. Honshu and Kyushu in forested areas near cliffs and caves.",
    echolocates: true,
};

pub const MYOTIS_FORMOSUS: BatSpecies = BatSpecies {
    id: "myotis_formosus",
    name: "Hodgson's Bat",
    scientific_name: "Myotis formosus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 95_000.0,
    description: "Steep downward FM calls. Distinctive orange-brown coloring. Tsushima Island and western Japan. Forest-dwelling insectivore.",
    echolocates: true,
};

pub const MYOTIS_HOSONOI: BatSpecies = BatSpecies {
    id: "myotis_hosonoi",
    name: "Hosono's Myotis",
    scientific_name: "Myotis hosonoi",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 95_000.0,
    description: "FM calls typical of Myotis. Endemic to Japan (Honshu). Cave-dwelling. Poorly studied.",
    echolocates: true,
};

pub const MYOTIS_GRACILIS: BatSpecies = BatSpecies {
    id: "myotis_gracilis",
    name: "Gracile Myotis",
    scientific_name: "Myotis gracilis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM calls typical of small Myotis. Endemic to Japan (Honshu). Forest-dwelling. Limited published acoustic data.",
    echolocates: true,
};

pub const MYOTIS_OZENSIS: BatSpecies = BatSpecies {
    id: "myotis_ozensis",
    name: "Ozensis Myotis",
    scientific_name: "Myotis ozensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 95_000.0,
    description: "FM calls typical of Myotis. Endemic to Japan (central Honshu mountains). Cave-dwelling. Limited distribution.",
    echolocates: true,
};

// ── Vespertilionidae: Pipistrellus ───────────────────────────────────────────

pub const PIPISTRELLUS_ABRAMUS: BatSpecies = BatSpecies {
    id: "pipistrellus_abramus",
    name: "Japanese Pipistrelle",
    scientific_name: "Pipistrellus abramus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 95_000.0,
    description: "FM sweeps ~80\u{2013}95 kHz to terminal ~40 kHz, peak ~52 kHz. Japan's most common urban bat. Roosts in buildings and under bridges. Forages around streetlights.",
    echolocates: true,
};

pub const PIPISTRELLUS_ENDOI: BatSpecies = BatSpecies {
    id: "pipistrellus_endoi",
    name: "Endo's Pipistrelle",
    scientific_name: "Pipistrellus endoi",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 90_000.0,
    description: "FM-QCF calls similar to P. abramus but slightly lower frequency. Endemic to Japan (Honshu). Temperate forests at 100\u{2013}1500 m. IUCN Near Threatened.",
    echolocates: true,
};

pub const PIPISTRELLUS_STURDEEI: BatSpecies = BatSpecies {
    id: "pipistrellus_sturdeei",
    name: "Sturdee's Pipistrelle",
    scientific_name: "Pipistrellus sturdeei",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 90_000.0,
    description: "Presumed FM-QCF calls. Known only from a specimen on the Bonin Islands (Ogasawara). Not seen since 1915\u{2014}possibly extinct.",
    echolocates: true,
};

// ── Vespertilionidae: Vespertilio ────────────────────────────────────────────

pub const VESPERTILIO_SINENSIS: BatSpecies = BatSpecies {
    id: "vespertilio_sinensis",
    name: "Asian Parti-coloured Bat",
    scientific_name: "Vespertilio sinensis",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 46_000.0,
    description: "FM-QCF calls, peak ~24 kHz, max ~46 kHz, min ~22\u{2013}23 kHz. Steep FM into shallow QCF tail. Migratory. Hokkaido to Kyushu. Roosts in tree hollows and buildings.",
    echolocates: true,
};

// ── Vespertilionidae: Eptesicus ──────────────────────────────────────────────

pub const EPTESICUS_NILSSONII_JP: BatSpecies = BatSpecies {
    id: "eptesicus_nilssonii_jp",
    name: "Northern Bat",
    scientific_name: "Eptesicus nilssonii",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM-QCF calls, peak ~30.5 kHz, duration ~6 ms, bandwidth ~32 kHz. Hokkaido and northern Honshu. The most northerly-ranging bat in the world.",
    echolocates: true,
};

pub const EPTESICUS_JAPONENSIS: BatSpecies = BatSpecies {
    id: "eptesicus_japonensis",
    name: "Japanese Short-tailed Bat",
    scientific_name: "Eptesicus japonensis",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM-QCF calls similar to E. nilssonii. Endemic to Japan (Honshu, Shikoku, Kyushu). Forest-dwelling, roosts in tree hollows and buildings.",
    echolocates: true,
};

// ── Vespertilionidae: Nyctalus ───────────────────────────────────────────────

pub const NYCTALUS_AVIATOR: BatSpecies = BatSpecies {
    id: "nyctalus_aviator",
    name: "Japanese Noctule",
    scientific_name: "Nyctalus aviator",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 17_000.0,
    freq_hi_hz: 54_000.0,
    description: "FM-QCF calls, peak ~21 kHz, duration ~12 ms. Japan's largest open-space insectivorous bat. Forages up to 300 m altitude. Occasionally preys on migrating birds. Roosts in tree cavities.",
    echolocates: true,
};

// ── Vespertilionidae: Plecotus ───────────────────────────────────────────────

pub const PLECOTUS_SACRIMONTIS: BatSpecies = BatSpecies {
    id: "plecotus_sacrimontis",
    name: "Japanese Long-eared Bat",
    scientific_name: "Plecotus sacrimontis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 23_000.0,
    freq_hi_hz: 46_000.0,
    description: "Low-intensity FM calls, start ~46 kHz, peak ~41 kHz, end ~23 kHz. Gleaning forager specialising in fluttering moths. Endemic to Japan (Honshu, Hokkaido, Shikoku, Kyushu).",
    echolocates: true,
};

// ── Vespertilionidae: Barbastella ────────────────────────────────────────────

pub const BARBASTELLA_LEUCOMELAS: BatSpecies = BatSpecies {
    id: "barbastella_leucomelas",
    name: "Asian Barbastelle",
    scientific_name: "Barbastella leucomelas",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 47_000.0,
    description: "FM calls in two alternating types: type A at 32\u{2013}37 kHz, type B at 38\u{2013}45 kHz. Similar to European B. barbastellus. Rare in Japan (Honshu). Forest-dwelling, very elusive.",
    echolocates: true,
};

// ── Vespertilionidae: Murina (tube-nosed bats) ──────────────────────────────

pub const MURINA_HILGENDORFI: BatSpecies = BatSpecies {
    id: "murina_hilgendorfi",
    name: "Hilgendorf's Tube-nosed Bat",
    scientific_name: "Murina hilgendorfi",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 165_000.0,
    description: "Ultra-broadband FM sweeps ~145\u{2013}165 kHz down to ~45\u{2013}55 kHz. Forest gleaner. Throughout Japan (Hokkaido to Kyushu). Roosts in tree hollows and dead curled leaves.",
    echolocates: true,
};

pub const MURINA_USSURIENSIS: BatSpecies = BatSpecies {
    id: "murina_ussuriensis",
    name: "Ussuri Tube-nosed Bat",
    scientific_name: "Murina ussuriensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 165_000.0,
    description: "Ultra-broadband FM sweeps similar to M. hilgendorfi. Tiny bat (4\u{2013}8 g). Remarkably hibernates under snow. Roosts in curled dead leaves. Hokkaido and Honshu.",
    echolocates: true,
};

pub const MURINA_SILVATICA: BatSpecies = BatSpecies {
    id: "murina_silvatica",
    name: "Ognev's Tube-nosed Bat",
    scientific_name: "Murina silvatica",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 160_000.0,
    description: "Broadband FM calls typical of Murina. Distribution spans ~2000 km north\u{2013}south across the Japanese archipelago. Forest bat roosting in dead curled leaves.",
    echolocates: true,
};

pub const MURINA_TENEBROSA: BatSpecies = BatSpecies {
    id: "murina_tenebrosa",
    name: "Gloomy Tube-nosed Bat",
    scientific_name: "Murina tenebrosa",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 160_000.0,
    description: "FM calls presumed similar to other Murina. Known only from a single holotype on Tsushima Island (1962). Possibly extinct. Alliance for Zero Extinction species.",
    echolocates: true,
};

pub const MURINA_RYUKYUANA: BatSpecies = BatSpecies {
    id: "murina_ryukyuana",
    name: "Ryukyu Tube-nosed Bat",
    scientific_name: "Murina ryukyuana",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 160_000.0,
    description: "Broadband FM calls typical of Murina. Endemic to the Ryukyu Islands. Recently described species from Okinawa. Forest-dwelling.",
    echolocates: true,
};

// ══════════════════════════════════════════════════════════════════════════════
// North America species (USA + Canada)
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Fenton & Bell (1981): Echolocation calls of NA bats
// - O'Farrell et al. (1999): Acoustic identification of NA insectivorous bats
// - Szewczak (2004): Advanced analysis techniques for bat echolocation calls
// - Various state acoustic ID guides (e.g. Bat Call ID project, SonoBat)
// - Holroyd et al. (2014): Canadian bat species & echolocation
// - Kunz & Fenton (2003): Bat Ecology

// ── Vespertilionidae ─────────────────────────────────────────────────────────

pub const MYOTIS_LUCIFUGUS: BatSpecies = BatSpecies {
    id: "myotis_lucifugus",
    name: "Little Brown Myotis",
    scientific_name: "Myotis lucifugus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 80_000.0,
    description: "Historically North America's most common bat, now severely impacted by White-nose Syndrome. FM sweep ~80\u{2013}40 kHz, characteristic frequency ~45 kHz. Forages over water and forest edges.",
    echolocates: true,
};

pub const MYOTIS_SEPTENTRIONALIS: BatSpecies = BatSpecies {
    id: "myotis_septentrionalis",
    name: "Northern Long-eared Myotis",
    scientific_name: "Myotis septentrionalis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 115_000.0,
    description: "Steep broadband FM sweeps, short duration. Characteristic frequency ~55 kHz. Gleaning forager in forest understory. Federally endangered due to White-nose Syndrome.",
    echolocates: true,
};

pub const MYOTIS_SODALIS: BatSpecies = BatSpecies {
    id: "myotis_sodalis",
    name: "Indiana Bat",
    scientific_name: "Myotis sodalis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 80_000.0,
    description: "Calls very similar to Little Brown Myotis; characteristic frequency ~45 kHz. Hibernates in dense clusters in limestone caves. Federally endangered.",
    echolocates: true,
};

pub const MYOTIS_GRISESCENS: BatSpecies = BatSpecies {
    id: "myotis_grisescens",
    name: "Gray Myotis",
    scientific_name: "Myotis grisescens",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 70_000.0,
    description: "Largest eastern Myotis. FM sweep ~70\u{2013}35 kHz, characteristic frequency ~45 kHz. Obligate cave bat year-round. Federally endangered but populations recovering.",
    echolocates: true,
};

pub const MYOTIS_LEIBII: BatSpecies = BatSpecies {
    id: "myotis_leibii",
    name: "Eastern Small-footed Myotis",
    scientific_name: "Myotis leibii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 90_000.0,
    description: "One of North America's smallest bats (~5 g). FM sweep with characteristic frequency ~50\u{2013}55 kHz. Roosts in rock crevices and talus slopes. Appears somewhat resistant to White-nose Syndrome.",
    echolocates: true,
};

pub const MYOTIS_VOLANS: BatSpecies = BatSpecies {
    id: "myotis_volans",
    name: "Long-legged Myotis",
    scientific_name: "Myotis volans",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 80_000.0,
    description: "Western species found in coniferous forests. FM sweep with characteristic frequency ~40 kHz. Keeled calcar and furred underwing membrane diagnostic. Fast, direct flight.",
    echolocates: true,
};

pub const MYOTIS_THYSANODES: BatSpecies = BatSpecies {
    id: "myotis_thysanodes",
    name: "Fringed Myotis",
    scientific_name: "Myotis thysanodes",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 55_000.0,
    description: "Named for the fringe of stiff hairs along the tail membrane. FM sweep with characteristic frequency ~40 kHz. Roosts in caves, mines, and buildings in western mountains.",
    echolocates: true,
};

pub const MYOTIS_EVOTIS: BatSpecies = BatSpecies {
    id: "myotis_evotis",
    name: "Long-eared Myotis",
    scientific_name: "Myotis evotis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 75_000.0,
    description: "Very quiet, short FM calls typical of a gleaning forager. Characteristic frequency ~40 kHz. Large ears extend well beyond nose when laid forward. Western forests and woodlands.",
    echolocates: true,
};

pub const MYOTIS_CALIFORNICUS: BatSpecies = BatSpecies {
    id: "myotis_californicus",
    name: "California Myotis",
    scientific_name: "Myotis californicus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 90_000.0,
    description: "Small western Myotis, highly adaptable. FM sweep with characteristic frequency ~50 kHz. Difficult to distinguish acoustically from Western Small-footed Myotis. Often the most common bat at western sites.",
    echolocates: true,
};

pub const MYOTIS_CILIOLABRUM: BatSpecies = BatSpecies {
    id: "myotis_ciliolabrum",
    name: "Western Small-footed Myotis",
    scientific_name: "Myotis ciliolabrum",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 90_000.0,
    description: "Tiny bat (~5 g) of western arid lands. FM sweep with characteristic frequency ~50 kHz. Calls nearly identical to California Myotis. Roosts in rock crevices, cliff faces, and badlands.",
    echolocates: true,
};

pub const MYOTIS_AUSTRORIPARIUS: BatSpecies = BatSpecies {
    id: "myotis_austroriparius",
    name: "Southeastern Myotis",
    scientific_name: "Myotis austroriparius",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 80_000.0,
    description: "Southeastern US cave bat, often found near water. FM sweep with characteristic frequency ~50 kHz. Forms large maternity colonies in caves, sometimes with thousands of individuals.",
    echolocates: true,
};

pub const MYOTIS_YUMANENSIS: BatSpecies = BatSpecies {
    id: "myotis_yumanensis",
    name: "Yuma Myotis",
    scientific_name: "Myotis yumanensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 80_000.0,
    description: "Western water-foraging specialist. FM sweep with characteristic frequency ~50 kHz. Trawls insects from water surfaces with large feet. Common near rivers and lakes.",
    echolocates: true,
};

pub const MYOTIS_VELIFER: BatSpecies = BatSpecies {
    id: "myotis_velifer",
    name: "Cave Myotis",
    scientific_name: "Myotis velifer",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 70_000.0,
    description: "Large Myotis of the south-central US and Mexico. FM sweep with characteristic frequency ~45 kHz. Forms large cave colonies. Bare patches on back between shoulder blades diagnostic.",
    echolocates: true,
};

pub const LASIURUS_BOREALIS: BatSpecies = BatSpecies {
    id: "lasiurus_borealis",
    name: "Eastern Red Bat",
    scientific_name: "Lasiurus borealis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 55_000.0,
    description: "Distinctive reddish fur. Solitary foliage-roosting tree bat. FM sweep with characteristic frequency ~40 kHz. Long-distance migrant. One of the most common bats in eastern North America.",
    echolocates: true,
};

pub const LASIURUS_CINEREUS: BatSpecies = BatSpecies {
    id: "lasiurus_cinereus",
    name: "Hoary Bat",
    scientific_name: "Lasiurus cinereus",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 28_000.0,
    description: "North America's largest bat (~30 g). Distinctive low-frequency QCF calls ~20\u{2013}25 kHz, easily identified on spectrograms. Long-distance migrant, solitary foliage rooster. Frosted brown fur with white-tipped hairs.",
    echolocates: true,
};

pub const LASIURUS_SEMINOLUS: BatSpecies = BatSpecies {
    id: "lasiurus_seminolus",
    name: "Seminole Bat",
    scientific_name: "Lasiurus seminolus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 50_000.0,
    description: "Southeastern counterpart of the Eastern Red Bat with deep mahogany fur. FM sweep with characteristic frequency ~40 kHz. Roosts in Spanish moss and pine foliage.",
    echolocates: true,
};

pub const LASIURUS_INTERMEDIUS: BatSpecies = BatSpecies {
    id: "lasiurus_intermedius",
    name: "Northern Yellow Bat",
    scientific_name: "Lasiurus intermedius",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 40_000.0,
    description: "Large yellowish tree bat of the southeastern coastal plain. FM-QCF calls with characteristic frequency ~30 kHz. Roosts in dead palm fronds and Spanish moss.",
    echolocates: true,
};

pub const LASIURUS_XANTHINUS: BatSpecies = BatSpecies {
    id: "lasiurus_xanthinus",
    name: "Western Yellow Bat",
    scientific_name: "Lasiurus xanthinus",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 40_000.0,
    description: "Southwestern desert species. FM-QCF calls with characteristic frequency ~30 kHz. Roosts in palm fronds. Range expanding northward with ornamental palm plantings.",
    echolocates: true,
};

pub const LASIONYCTERIS_NOCTIVAGANS: BatSpecies = BatSpecies {
    id: "lasionycteris_noctivagans",
    name: "Silver-haired Bat",
    scientific_name: "Lasionycteris noctivagans",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 35_000.0,
    description: "Medium-sized bat with distinctive silver-tipped dark fur. Low-frequency QCF calls ~25\u{2013}27 kHz characteristic frequency. Slow, maneuverable flight. Solitary tree-roosting migrant. One of the most frequently killed bats at wind turbines.",
    echolocates: true,
};

pub const PERIMYOTIS_SUBFLAVUS: BatSpecies = BatSpecies {
    id: "perimyotis_subflavus",
    name: "Tricolored Bat",
    scientific_name: "Perimyotis subflavus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 55_000.0,
    description: "Tiny bat (~6 g) formerly called Eastern Pipistrelle. FM sweep with characteristic frequency ~45 kHz. Distinctive tricolored fur bands. Severely affected by White-nose Syndrome; proposed for ESA listing.",
    echolocates: true,
};

pub const PARASTRELLUS_HESPERUS: BatSpecies = BatSpecies {
    id: "parastrellus_hesperus",
    name: "Canyon Bat",
    scientific_name: "Parastrellus hesperus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 43_000.0,
    freq_hi_hz: 70_000.0,
    description: "Smallest bat in North America (~3.5 g), formerly called Western Pipistrelle. FM sweep with characteristic frequency ~50 kHz. Common in desert canyons and rock outcrops. Often the first bat seen at dusk.",
    echolocates: true,
};

pub const NYCTICEIUS_HUMERALIS: BatSpecies = BatSpecies {
    id: "nycticeius_humeralis",
    name: "Evening Bat",
    scientific_name: "Nycticeius humeralis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 45_000.0,
    description: "Medium-sized bat of the eastern US. FM-QCF calls with characteristic frequency ~35 kHz. Resembles Big Brown Bat but smaller. Roosts in tree cavities and buildings. Does not use caves.",
    echolocates: true,
};

pub const CORYNORHINUS_TOWNSENDII: BatSpecies = BatSpecies {
    id: "corynorhinus_townsendii",
    name: "Townsend's Big-eared Bat",
    scientific_name: "Corynorhinus townsendii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 50_000.0,
    description: "Enormous ears (~35 mm). Very quiet, short FM calls for gleaning moths from foliage. Characteristic frequency ~25\u{2013}40 kHz. Highly sensitive to roost disturbance. Several subspecies are endangered.",
    echolocates: true,
};

pub const CORYNORHINUS_RAFINESQUII: BatSpecies = BatSpecies {
    id: "corynorhinus_rafinesquii",
    name: "Rafinesque's Big-eared Bat",
    scientific_name: "Corynorhinus rafinesquii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 50_000.0,
    description: "Southeastern counterpart of Townsend's Big-eared Bat. Very quiet gleaning calls, characteristic frequency ~25\u{2013}40 kHz. Roosts in abandoned buildings, hollow trees, and under bridges. White belly fur distinctive.",
    echolocates: true,
};

pub const IDIONYCTERIS_PHYLLOTIS: BatSpecies = BatSpecies {
    id: "idionycteris_phyllotis",
    name: "Allen's Big-eared Bat",
    scientific_name: "Idionycteris phyllotis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 9_000.0,
    freq_hi_hz: 25_000.0,
    description: "Distinctive low-frequency calls ~12\u{2013}15 kHz, often audible to humans. Large lappets projecting from base of ears. Southwestern pine and oak forests. Calls unlike any other NA Myotis-sized bat.",
    echolocates: true,
};

pub const EUDERMA_MACULATUM: BatSpecies = BatSpecies {
    id: "euderma_maculatum",
    name: "Spotted Bat",
    scientific_name: "Euderma maculatum",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 8_000.0,
    freq_hi_hz: 15_000.0,
    description: "Lowest echolocation frequency of any North American bat (~10\u{2013}12 kHz), audible to humans. Unmistakable: three large white spots on black dorsal fur, enormous pink ears. Rare but widespread in western arid lands.",
    echolocates: true,
};

pub const ANTROZOUS_PALLIDUS: BatSpecies = BatSpecies {
    id: "antrozous_pallidus",
    name: "Pallid Bat",
    scientific_name: "Antrozous pallidus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 55_000.0,
    description: "Unique dual foraging strategy: echolocates for aerial prey and uses passive listening to glean scorpions and large insects from the ground. FM calls ~30 kHz characteristic. Large ears, pale fur. Immune to scorpion venom.",
    echolocates: true,
};

// ── Molossidae ───────────────────────────────────────────────────────────────

pub const NYCTINOMOPS_MACROTIS: BatSpecies = BatSpecies {
    id: "nyctinomops_macrotis",
    name: "Big Free-tailed Bat",
    scientific_name: "Nyctinomops macrotis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 20_000.0,
    description: "Large molossid with low-frequency QCF calls ~14\u{2013}17 kHz, often audible. Roosts in high cliff crevices. Southwestern US. Loud, piercing social calls can be heard from considerable distance.",
    echolocates: true,
};

pub const NYCTINOMOPS_FEMOROSACCUS: BatSpecies = BatSpecies {
    id: "nyctinomops_femorosaccus",
    name: "Pocketed Free-tailed Bat",
    scientific_name: "Nyctinomops femorosaccus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 16_000.0,
    freq_hi_hz: 25_000.0,
    description: "Medium-sized free-tailed bat of the southwestern deserts. QCF calls ~22 kHz characteristic frequency. Named for the pocket-like fold on the tail membrane. Roosts in rock crevices.",
    echolocates: true,
};

pub const EUMOPS_PEROTIS: BatSpecies = BatSpecies {
    id: "eumops_perotis",
    name: "Western Mastiff Bat",
    scientific_name: "Eumops perotis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 7_000.0,
    freq_hi_hz: 18_000.0,
    description: "Largest bat in North America (wingspan ~56 cm). Very low-frequency QCF calls ~10\u{2013}12 kHz, easily audible to humans. Needs vertical drop to launch into flight. Roosts in tall cliff faces and buildings.",
    echolocates: true,
};

pub const EUMOPS_FLORIDANUS: BatSpecies = BatSpecies {
    id: "eumops_floridanus",
    name: "Florida Bonneted Bat",
    scientific_name: "Eumops floridanus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 8_000.0,
    freq_hi_hz: 20_000.0,
    description: "Federally endangered, restricted to southern Florida. Low-frequency QCF calls ~14\u{2013}16 kHz. One of the rarest bats in North America. Roosts in tree cavities and bat houses. Occasionally audible to humans.",
    echolocates: true,
};

// ── Phyllostomidae ───────────────────────────────────────────────────────────

pub const MACROTUS_CALIFORNICUS: BatSpecies = BatSpecies {
    id: "macrotus_californicus",
    name: "California Leaf-nosed Bat",
    scientific_name: "Macrotus californicus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 90_000.0,
    description: "Low-intensity gleaning calls ~50 kHz, difficult to detect with bat detectors. Large ears and prominent nose leaf. Non-migratory desert species of AZ and southern CA. Cannot hibernate; relies on warm roost sites year-round.",
    echolocates: true,
};

pub const CHOERONYCTERIS_MEXICANA: BatSpecies = BatSpecies {
    id: "choeronycteris_mexicana",
    name: "Mexican Long-tongued Bat",
    scientific_name: "Choeronycteris mexicana",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 90_000.0,
    description: "Nectar-feeding bat with elongated snout and long tongue. Very quiet FM calls ~75 kHz. Seasonal migrant to southern AZ. Pollinates agave and columnar cacti. Low-intensity echolocation typical of phyllostomids.",
    echolocates: true,
};

pub const LEPTONYCTERIS_YERBABUENAE: BatSpecies = BatSpecies {
    id: "leptonycteris_yerbabuenae",
    name: "Lesser Long-nosed Bat",
    scientific_name: "Leptonycteris yerbabuenae",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 90_000.0,
    description: "Nectar bat, major pollinator of saguaro and organ pipe cacti. Quiet FM calls ~75 kHz. Migrates to southern AZ in summer. Formerly endangered, delisted in 2018 \u{2014} a conservation success story.",
    echolocates: true,
};

pub const LEPTONYCTERIS_NIVALIS: BatSpecies = BatSpecies {
    id: "leptonycteris_nivalis",
    name: "Mexican Long-nosed Bat",
    scientific_name: "Leptonycteris nivalis",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 65_000.0,
    freq_hi_hz: 85_000.0,
    description: "Federally endangered nectar bat. Migrates to Big Bend region of Texas in summer. Pollinates agave plants. Quiet FM calls ~75 kHz. Only known US roost in a single cave in the Chisos Mountains.",
    echolocates: true,
};

// ══════════════════════════════════════════════════════════════════════════════
// South America additional species
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - López-Baucells et al. (2016) Pelagic: Guía de los murciélagos de la Amazonia
// - Barataud (2020): Ecologie acoustique des chiroptères d'Europe (neotropical appendix)
// - Jung et al. (2014) PMC: Neotropical molossid call design
// - Arias-Aguilar et al. (2018): Echolocation of Amazonian bats
// - Schnitzler & Kalko (2001): Echolocation by insect-eating bats
// - Nogueira et al. (2014): Echolocation of Brazilian bats
// - Falcão et al. (2015): Bat assemblages in Brazilian Atlantic Forest
// - Rodríguez-San Pedro & Simonetti (2015): Chilean bats

// ── Mormoopidae ────────────────────────────────────────────────────────────

pub const PTERONOTUS_PARNELLII: BatSpecies = BatSpecies {
    id: "pteronotus_parnellii",
    name: "Parnell's Mustached Bat",
    scientific_name: "Pteronotus parnellii",
    family: "Mormoopidae",
    call_type: "CF-FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 120_000.0,
    description: "The South American high-duty-cycle echolocator. Long CF at ~60 kHz (2nd harmonic) with Doppler compensation. Split from P. mesoamericanus. Huge cave colonies.",
    echolocates: true,
};

// ── Furipteridae (endemic Neotropical family) ──────────────────────────────

pub const FURIPTERUS_HORRENS: BatSpecies = BatSpecies {
    id: "furipterus_horrens",
    name: "Thumbless Bat",
    scientific_name: "Furipterus horrens",
    family: "Furipteridae",
    call_type: "FM",
    freq_lo_hz: 90_000.0,
    freq_hi_hz: 160_000.0,
    description: "Tiny (3\u{2013}5 g) insectivore with vestigial thumb. Very high frequency FM sweeps peaking ~130\u{2013}150 kHz. One of the highest-frequency New World bats. Caves and mines.",
    echolocates: true,
};

pub const AMORPHOCHILUS_SCHNABLII: BatSpecies = BatSpecies {
    id: "amorphochilus_schnablii",
    name: "Smoky Bat",
    scientific_name: "Amorphochilus schnablii",
    family: "Furipteridae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 120_000.0,
    description: "Rare, endemic to western South America (Ecuador, Peru, Chile). High-frequency FM sweeps ~80\u{2013}100 kHz. Cave-dwelling. IUCN Vulnerable.",
    echolocates: true,
};

// ── Natalidae ──────────────────────────────────────────────────────────────

pub const NATALUS_MACROURUS: BatSpecies = BatSpecies {
    id: "natalus_macrourus",
    name: "Brazilian Funnel-eared Bat",
    scientific_name: "Natalus macrourus",
    family: "Natalidae",
    call_type: "FM",
    freq_lo_hz: 80_000.0,
    freq_hi_hz: 150_000.0,
    description: "Very high frequency echolocator, peak ~100\u{2013}120 kHz. Extremely low intensity. Cave-roosting. Endemic to eastern Brazil (Cerrado/Caatinga). IUCN Vulnerable.",
    echolocates: true,
};

pub const NATALUS_TUMIDIROSTRIS: BatSpecies = BatSpecies {
    id: "natalus_tumidirostris",
    name: "Trinidadian Funnel-eared Bat",
    scientific_name: "Natalus tumidirostris",
    family: "Natalidae",
    call_type: "FM",
    freq_lo_hz: 80_000.0,
    freq_hi_hz: 140_000.0,
    description: "Very high frequency FM, peak ~100\u{2013}130 kHz. Cave-roosting. Northern South America (Venezuela, Colombia, Trinidad). Low-intensity calls barely detectable beyond 1 m.",
    echolocates: true,
};

// ── Thyropteridae ──────────────────────────────────────────────────────────

pub const THYROPTERA_DISCIFERA: BatSpecies = BatSpecies {
    id: "thyroptera_discifera",
    name: "Peters' Disk-winged Bat",
    scientific_name: "Thyroptera discifera",
    family: "Thyropteridae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 80_000.0,
    description: "Similar to T. tricolor but slightly larger suction disks. Peak ~50 kHz. Roosts in furled Heliconia leaves. Amazonian lowland forests. Extremely low-intensity echolocation.",
    echolocates: true,
};

pub const THYROPTERA_LAVALI: BatSpecies = BatSpecies {
    id: "thyroptera_lavali",
    name: "LaVal's Disk-winged Bat",
    scientific_name: "Thyroptera lavali",
    family: "Thyropteridae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 75_000.0,
    description: "Rare Amazonian disk-winged bat. Roosts in curled leaves. Poorly known acoustically. Low-intensity FM calls.",
    echolocates: true,
};

// ── Emballonuridae ─────────────────────────────────────────────────────────

pub const SACCOPTERYX_CANESCENS: BatSpecies = BatSpecies {
    id: "saccopteryx_canescens",
    name: "Frosted Sac-winged Bat",
    scientific_name: "Saccopteryx canescens",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 55_000.0,
    description: "Amazonian endemic. Highest frequency Saccopteryx (~52 kHz). Similar QCF structure to congeners. Smaller than S. bilineata. Roosts on tree trunks in terra firme forest.",
    echolocates: true,
};

pub const CENTRONYCTERIS_MAXIMILIANI: BatSpecies = BatSpecies {
    id: "centronycteris_maximiliani",
    name: "Shaggy Bat",
    scientific_name: "Centronycteris maximiliani",
    family: "Emballonuridae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 65_000.0,
    description: "Rare canopy-level forager. Steep FM sweeps ~45\u{2013}55 kHz. Long shaggy fur. One of the least-known Neotropical emballonurids. Amazonian forests.",
    echolocates: true,
};

pub const PEROPTERYX_LEUCOPTERA: BatSpecies = BatSpecies {
    id: "peropteryx_leucoptera",
    name: "White-winged Dog-like Bat",
    scientific_name: "Peropteryx leucoptera",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 50_000.0,
    description: "Distinctive white wing membrane patches. QCF at ~42 kHz. Amazonian lowland forests.",
    echolocates: true,
};

pub const PEROPTERYX_TRINITATIS: BatSpecies = BatSpecies {
    id: "peropteryx_trinitatis",
    name: "Trinidad Dog-like Bat",
    scientific_name: "Peropteryx trinitatis",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 52_000.0,
    description: "Northern South America. QCF at ~43 kHz. Open-area forager near rock shelters and caves.",
    echolocates: true,
};

// ── Phyllostomidae (additional South American species) ─────────────────────

pub const DIPHYLLA_ECAUDATA: BatSpecies = BatSpecies {
    id: "diphylla_ecaudata",
    name: "Hairy-legged Vampire Bat",
    scientific_name: "Diphylla ecaudata",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 65_000.0,
    freq_hi_hz: 100_000.0,
    description: "Bird blood specialist (unlike the mammal-feeding D. rotundus). Peak ~80 kHz. Higher frequency than common vampire. Short multiharmonic FM. Low-intensity.",
    echolocates: true,
};

pub const DIAEMUS_YOUNGI: BatSpecies = BatSpecies {
    id: "diaemus_youngi",
    name: "White-winged Vampire Bat",
    scientific_name: "Diaemus youngi",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 95_000.0,
    description: "Feeds on bird blood. Peak ~70 kHz. White wing tips distinctive in flight. Low-intensity FM. Rarer than D. rotundus. Lowland forests.",
    echolocates: true,
};

pub const ARTIBEUS_OBSCURUS: BatSpecies = BatSpecies {
    id: "artibeus_obscurus",
    name: "Dark Fruit-eating Bat",
    scientific_name: "Artibeus obscurus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 85_000.0,
    description: "Common Amazonian frugivore. Peak ~55 kHz. Intermediate size between A. jamaicensis and A. lituratus. Low-intensity FM. Indistinct facial stripes.",
    echolocates: true,
};

pub const ARTIBEUS_PLANIROSTRIS: BatSpecies = BatSpecies {
    id: "artibeus_planirostris",
    name: "Flat-faced Fruit Bat",
    scientific_name: "Artibeus planirostris",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 82_000.0,
    description: "Very common South American frugivore, replaces A. jamaicensis in many mainland habitats. Peak ~54 kHz. Low-intensity multiharmonic FM. Important seed disperser.",
    echolocates: true,
};

pub const PLATYRRHINUS_LINEATUS: BatSpecies = BatSpecies {
    id: "platyrrhinus_lineatus",
    name: "White-lined Broad-nosed Bat",
    scientific_name: "Platyrrhinus lineatus",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 90_000.0,
    description: "Common frugivore of open habitats and forest edges. Peak ~62 kHz. Prominent white facial and dorsal stripes. Low-intensity FM. Cerrado and Atlantic Forest.",
    echolocates: true,
};

pub const PLATYRRHINUS_HELLERI: BatSpecies = BatSpecies {
    id: "platyrrhinus_helleri",
    name: "Heller's Broad-nosed Bat",
    scientific_name: "Platyrrhinus helleri",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 95_000.0,
    description: "Small frugivore. Peak ~68 kHz. Low-intensity FM. Widespread in Neotropical lowland forests. Important seed disperser.",
    echolocates: true,
};

pub const VAMPYRESSA_THYONE: BatSpecies = BatSpecies {
    id: "vampyressa_thyone",
    name: "Northern Little Yellow-eared Bat",
    scientific_name: "Vampyressa thyone",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 105_000.0,
    description: "Small frugivore. Peak ~78 kHz. Low-intensity FM. Tent-roosting. Yellow ear margins. Northern South American lowlands.",
    echolocates: true,
};

pub const VAMPYRODES_CARACCIOLI: BatSpecies = BatSpecies {
    id: "vampyrodes_caraccioli",
    name: "Great Stripe-faced Bat",
    scientific_name: "Vampyrodes caraccioli",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 48_000.0,
    freq_hi_hz: 82_000.0,
    description: "Large stenodermatine frugivore. Peak ~58 kHz. Prominent white facial stripes. Low-intensity FM. Forages in canopy.",
    echolocates: true,
};

pub const LONCHOPHYLLA_THOMASI: BatSpecies = BatSpecies {
    id: "lonchophylla_thomasi",
    name: "Thomas's Nectar Bat",
    scientific_name: "Lonchophylla thomasi",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 110_000.0,
    description: "Small nectarivore. Peak ~80 kHz. Low-intensity multiharmonic FM. Important pollinator of understory plants. Amazonian forests.",
    echolocates: true,
};

pub const ANOURA_CAUDIFER: BatSpecies = BatSpecies {
    id: "anoura_caudifer",
    name: "Tailed Tailless Bat",
    scientific_name: "Anoura caudifer",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 100_000.0,
    description: "Nectarivore with short tail (unlike tailless A. geoffroyi). Peak ~72 kHz. Low-intensity FM. Atlantic Forest and lower Andean slopes. Important pollinator.",
    echolocates: true,
};

pub const ANOURA_CULTRATA: BatSpecies = BatSpecies {
    id: "anoura_cultrata",
    name: "Handley's Tailless Bat",
    scientific_name: "Anoura cultrata",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 95_000.0,
    description: "Highland nectarivore. Peak ~68 kHz. Low-intensity FM. Andean cloud forests 1000\u{2013}2600 m. Uniquely keeled lower incisors.",
    echolocates: true,
};

pub const STURNIRA_ERYTHROMOS: BatSpecies = BatSpecies {
    id: "sturnira_erythromos",
    name: "Hairy Yellow-shouldered Bat",
    scientific_name: "Sturnira erythromos",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 62_000.0,
    freq_hi_hz: 80_000.0,
    description: "Andean frugivore of montane forests (1000\u{2013}3200 m). Peak ~70 kHz. Low-intensity FM. Key seed disperser in cloud forest ecosystems.",
    echolocates: true,
};

pub const STURNIRA_BOGOTENSIS: BatSpecies = BatSpecies {
    id: "sturnira_bogotensis",
    name: "Bogota Yellow-shouldered Bat",
    scientific_name: "Sturnira bogotensis",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 78_000.0,
    description: "High-altitude Andean frugivore (2000\u{2013}3400 m). Peak ~68 kHz. Low-intensity FM. One of the few bat species found above 3000 m.",
    echolocates: true,
};

pub const CHIRODERMA_VILLOSUM: BatSpecies = BatSpecies {
    id: "chiroderma_villosum",
    name: "Hairy Big-eyed Bat",
    scientific_name: "Chiroderma villosum",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 90_000.0,
    description: "Canopy frugivore with large eyes. Peak ~62 kHz. Low-intensity FM. Widespread in Neotropical lowlands. White dorsal stripe.",
    echolocates: true,
};

pub const PHYLLODERMA_STENOPS: BatSpecies = BatSpecies {
    id: "phylloderma_stenops",
    name: "Pale-faced Bat",
    scientific_name: "Phylloderma stenops",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 80_000.0,
    description: "Large omnivorous phyllostomid. Peak ~55 kHz. Low-intensity FM. Pale face distinctive. Roosts in hollow trees. Amazonian forests.",
    echolocates: true,
};

pub const RHINOPHYLLA_PUMILIO: BatSpecies = BatSpecies {
    id: "rhinophylla_pumilio",
    name: "Dwarf Little Fruit Bat",
    scientific_name: "Rhinophylla pumilio",
    family: "Phyllostomidae",
    call_type: "FM",
    freq_lo_hz: 65_000.0,
    freq_hi_hz: 110_000.0,
    description: "Small Amazonian frugivore. Peak ~80 kHz. Low-intensity FM. Related to Carollia. Important disperser of understory fruits. Common in terra firme forest.",
    echolocates: true,
};

// ── Molossidae (additional South American species) ─────────────────────────

pub const MOLOSSOPS_TEMMINCKII: BatSpecies = BatSpecies {
    id: "molossops_temminckii",
    name: "Dwarf Dog-faced Bat",
    scientific_name: "Molossops temminckii",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 50_000.0,
    description: "Small molossid. QCF at ~38\u{2013}42 kHz. One of the highest frequency molossids, consistent with small body size. Open cerrado and forest edge forager.",
    echolocates: true,
};

pub const MOLOSSOPS_NEGLECTUS: BatSpecies = BatSpecies {
    id: "molossops_neglectus",
    name: "Rufous Dog-faced Bat",
    scientific_name: "Molossops neglectus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 32_000.0,
    freq_hi_hz: 48_000.0,
    description: "Small Amazonian molossid. QCF at ~40 kHz. Poorly known. Forest edge and gap forager.",
    echolocates: true,
};

pub const EUMOPS_BONARIENSIS: BatSpecies = BatSpecies {
    id: "eumops_bonariensis",
    name: "Peters' Mastiff Bat",
    scientific_name: "Eumops bonariensis",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 34_000.0,
    description: "Medium-sized bonneted bat. QCF at ~26\u{2013}30 kHz. Open-area forager. Southern South America (Argentina, Uruguay, Brazil). Roosts in buildings.",
    echolocates: true,
};

pub const EUMOPS_HANSAE: BatSpecies = BatSpecies {
    id: "eumops_hansae",
    name: "Sanborn's Bonneted Bat",
    scientific_name: "Eumops hansae",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 24_000.0,
    freq_hi_hz: 38_000.0,
    description: "Medium molossid. QCF at ~28\u{2013}32 kHz. Amazonian and Atlantic Forest lowlands. Roosts in tree hollows. Uncommonly recorded.",
    echolocates: true,
};

pub const EUMOPS_DABBENEI: BatSpecies = BatSpecies {
    id: "eumops_dabbenei",
    name: "Big Bonneted Bat",
    scientific_name: "Eumops dabbenei",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 14_000.0,
    freq_hi_hz: 24_000.0,
    description: "Very large molossid. Low-frequency QCF at ~18\u{2013}20 kHz, sometimes audible. Open-space forager over savanna and cerrado. Northern Argentina to Colombia.",
    echolocates: true,
};

pub const EUMOPS_PATAGONICUS: BatSpecies = BatSpecies {
    id: "eumops_patagonicus",
    name: "Patagonian Bonneted Bat",
    scientific_name: "Eumops patagonicus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 36_000.0,
    description: "Medium bonneted bat of the southern cone (Argentina, Paraguay, southern Brazil). QCF at ~28 kHz. Open and semi-open habitat forager. Roosts in buildings and tree hollows.",
    echolocates: true,
};

pub const PROMOPS_NASUTUS: BatSpecies = BatSpecies {
    id: "promops_nasutus",
    name: "Brown Mastiff Bat",
    scientific_name: "Promops nasutus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 38_000.0,
    description: "QCF at ~30 kHz. Similar to P. centralis but lacks the distinctive upward frequency modulation. Open-space forager. South American drylands and forest edges.",
    echolocates: true,
};

pub const CYNOMOPS_ABRASUS: BatSpecies = BatSpecies {
    id: "cynomops_abrasus",
    name: "Cinnamon Dog-faced Bat",
    scientific_name: "Cynomops abrasus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 28_000.0,
    description: "Medium molossid. Low-frequency QCF at ~22\u{2013}24 kHz. Open-space forager. Brazilian cerrado and Atlantic Forest edge.",
    echolocates: true,
};

pub const CYNOMOPS_PLANIROSTRIS: BatSpecies = BatSpecies {
    id: "cynomops_planirostris",
    name: "Southern Dog-faced Bat",
    scientific_name: "Cynomops planirostris",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 32_000.0,
    description: "Small to medium molossid. QCF at ~25 kHz. Flat face with forward-pointing nostrils. Open habitats across South American lowlands.",
    echolocates: true,
};

// ── Vespertilionidae (additional South American species) ───────────────────

pub const MYOTIS_ALBESCENS: BatSpecies = BatSpecies {
    id: "myotis_albescens",
    name: "Silver-tipped Myotis",
    scientific_name: "Myotis albescens",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 80_000.0,
    description: "Common South American Myotis. Peak ~48 kHz. FM sweeps. Silver-tipped dorsal fur distinctive. Forages over water and in forest clearings.",
    echolocates: true,
};

pub const MYOTIS_CHILOENSIS: BatSpecies = BatSpecies {
    id: "myotis_chiloensis",
    name: "Chilean Myotis",
    scientific_name: "Myotis chiloensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 75_000.0,
    description: "Southern South American Myotis (Chile, Argentina, Patagonia). Peak ~45 kHz. FM sweeps. One of the southernmost-ranging bats in the Americas.",
    echolocates: true,
};

pub const MYOTIS_LEVIS: BatSpecies = BatSpecies {
    id: "myotis_levis",
    name: "Yellowish Myotis",
    scientific_name: "Myotis levis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 75_000.0,
    description: "South American Myotis. Peak ~46 kHz. FM sweeps. Southern Brazil, Uruguay, Argentina. Open areas and forest edges.",
    echolocates: true,
};

pub const MYOTIS_OXYOTUS: BatSpecies = BatSpecies {
    id: "myotis_oxyotus",
    name: "Montane Myotis",
    scientific_name: "Myotis oxyotus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 80_000.0,
    description: "High-altitude Andean Myotis (1500\u{2013}3800 m). Peak ~50 kHz. FM sweeps. Cloud forests and paramo edges. Colombia to Bolivia.",
    echolocates: true,
};

pub const MYOTIS_LAVALI: BatSpecies = BatSpecies {
    id: "myotis_lavali",
    name: "LaVal's Myotis",
    scientific_name: "Myotis lavali",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 42_000.0,
    freq_hi_hz: 85_000.0,
    description: "Small Myotis endemic to eastern Brazil (Cerrado, Caatinga). Peak ~52 kHz. Steep FM sweeps. Recently described. Associated with rock outcrops.",
    echolocates: true,
};

pub const LASIURUS_VILLOSISSIMUS: BatSpecies = BatSpecies {
    id: "lasiurus_villosissimus",
    name: "South American Hoary Bat",
    scientific_name: "Lasiurus villosissimus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 35_000.0,
    description: "Large vespertilionid, recently split from L. cinereus. Low-frequency FM-QCF at ~22\u{2013}25 kHz. Long narrow wings for fast open-air flight. Migratory. Frosted fur.",
    echolocates: true,
};

pub const HISTIOTUS_MONTANUS: BatSpecies = BatSpecies {
    id: "histiotus_montanus",
    name: "Small Big-eared Brown Bat",
    scientific_name: "Histiotus montanus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 45_000.0,
    description: "Distinctive very large ears (Plecotus-like). Peak ~28 kHz. FM sweeps. Open habitats in southern South America (Patagonia to southern Brazil). Slow, maneuverable flight.",
    echolocates: true,
};

pub const HISTIOTUS_VELATUS: BatSpecies = BatSpecies {
    id: "histiotus_velatus",
    name: "Tropical Big-eared Brown Bat",
    scientific_name: "Histiotus velatus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 50_000.0,
    description: "Large-eared vespertilionid. Peak ~32 kHz. FM sweeps. Brazilian cerrado and Atlantic Forest. Slightly higher frequency than H. montanus, consistent with smaller ears.",
    echolocates: true,
};

pub const HISTIOTUS_MACROTUS: BatSpecies = BatSpecies {
    id: "histiotus_macrotus",
    name: "Big-eared Brown Bat",
    scientific_name: "Histiotus macrotus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 16_000.0,
    freq_hi_hz: 40_000.0,
    description: "Largest-eared Histiotus. Very low-frequency peak ~25 kHz. FM sweeps. Chile and western Argentina. Arid and semi-arid open habitats. Gleans insects from foliage.",
    echolocates: true,
};

pub const EPTESICUS_DIMINUTUS: BatSpecies = BatSpecies {
    id: "eptesicus_diminutus",
    name: "Diminutive Serotine",
    scientific_name: "Eptesicus diminutus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 70_000.0,
    description: "Small Eptesicus of southern Brazil, Paraguay, Argentina. Peak ~50 kHz. FM-QCF. Forest edges and open areas.",
    echolocates: true,
};

// ══════════════════════════════════════════════════════════════════════════════
// Africa — species-level entries
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Monadjem et al. (2017) Acta Chiropt.: Acoustic Call Library for Swaziland
// - Monadjem et al. (2020) Bats of Southern and Central Africa (Wits Univ. Press)
// - Taylor et al. (2012) PLoS ONE: Rhinolophus hildebrandtii complex
// - Webala et al. (2019) J Bat Research & Conservation: Hipposideridae & Rhinonycteridae of Kenya
// - Jacobs et al. (2007, 2013, 2016, 2017): Echolocation allometry & geographic variation
// - Happold & Happold (2013) Mammals of Africa Vol. IV
// - ACR (African Chiroptera Report)
// - Fenton & Bell (1981): Cloeotis percivali 212 kHz
// - Holland et al. (2004): Rousettus aegyptiacus click echolocation

// ── Rhinolophidae ──

pub const RHINOLOPHUS_CLIVOSUS: BatSpecies = BatSpecies {
    id: "rhinolophus_clivosus",
    name: "Geoffroy's Horseshoe Bat",
    scientific_name: "Rhinolophus clivosus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 86_000.0,
    freq_hi_hz: 95_000.0,
    description: "Widespread in caves across southern and eastern Africa. CF peak varies geographically (~90\u{2013}92 kHz). Often roosts with Miniopterus.",
    echolocates: true,
};

pub const RHINOLOPHUS_CAPENSIS: BatSpecies = BatSpecies {
    id: "rhinolophus_capensis",
    name: "Cape Horseshoe Bat",
    scientific_name: "Rhinolophus capensis",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 80_000.0,
    freq_hi_hz: 88_000.0,
    description: "Endemic to South Africa (Western, Eastern, Northern Cape). CF peak ~83\u{2013}86 kHz. Coastal caves and rocky outcrops.",
    echolocates: true,
};

pub const RHINOLOPHUS_SIMULATOR: BatSpecies = BatSpecies {
    id: "rhinolophus_simulator",
    name: "Bushveld Horseshoe Bat",
    scientific_name: "Rhinolophus simulator",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 76_000.0,
    freq_hi_hz: 84_000.0,
    description: "Southern and eastern Africa savannas. CF peak ~80 kHz. Often sympatric with R. swinnyi.",
    echolocates: true,
};

pub const RHINOLOPHUS_SWINNYI: BatSpecies = BatSpecies {
    id: "rhinolophus_swinnyi",
    name: "Swinny's Horseshoe Bat",
    scientific_name: "Rhinolophus swinnyi",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 102_000.0,
    freq_hi_hz: 112_000.0,
    description: "Eastern South Africa, Eswatini, Zimbabwe. High CF peak ~107 kHz. Clutter forager in dense vegetation.",
    echolocates: true,
};

pub const RHINOLOPHUS_HILDEBRANDTII: BatSpecies = BatSpecies {
    id: "rhinolophus_hildebrandtii",
    name: "Hildebrandt's Horseshoe Bat",
    scientific_name: "Rhinolophus hildebrandtii",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 32_000.0,
    freq_hi_hz: 46_000.0,
    description: "Large horseshoe bat of east and southern Africa. CF peak varies 32\u{2013}46 kHz (species complex). Low frequency for a rhinolophid due to large body size.",
    echolocates: true,
};

pub const RHINOLOPHUS_FUMIGATUS: BatSpecies = BatSpecies {
    id: "rhinolophus_fumigatus",
    name: "R\u{fc}ppell's Horseshoe Bat",
    scientific_name: "Rhinolophus fumigatus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 66_000.0,
    description: "Sub-Saharan Africa woodlands and savanna. CF peak geographically variable: ~53\u{2013}59 kHz (southern), ~62\u{2013}66 kHz (Cameroon).",
    echolocates: true,
};

pub const RHINOLOPHUS_BLASII: BatSpecies = BatSpecies {
    id: "rhinolophus_blasii",
    name: "Blasius's Horseshoe Bat",
    scientific_name: "Rhinolophus blasii",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 82_000.0,
    freq_hi_hz: 90_000.0,
    description: "North Africa and East Africa. CF peak ~86 kHz. Mediterranean-type habitats and caves.",
    echolocates: true,
};

pub const RHINOLOPHUS_DARLINGI: BatSpecies = BatSpecies {
    id: "rhinolophus_darlingi",
    name: "Darling's Horseshoe Bat",
    scientific_name: "Rhinolophus darlingi",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 82_000.0,
    freq_hi_hz: 92_000.0,
    description: "Southern Africa woodlands (Zimbabwe, Mozambique, South Africa). Small species; CF peak ~86\u{2013}88 kHz. Rocky habitats and caves.",
    echolocates: true,
};

// ── Hipposideridae ──

pub const HIPPOSIDEROS_CAFFER: BatSpecies = BatSpecies {
    id: "hipposideros_caffer",
    name: "Sundevall's Roundleaf Bat",
    scientific_name: "Hipposideros caffer",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 128_000.0,
    freq_hi_hz: 153_000.0,
    description: "Most widespread African hipposiderid. High CF calls; frequency varies geographically. Caves, mines, buildings.",
    echolocates: true,
};

pub const HIPPOSIDEROS_RUBER: BatSpecies = BatSpecies {
    id: "hipposideros_ruber",
    name: "Noack's Roundleaf Bat",
    scientific_name: "Hipposideros ruber",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 121_000.0,
    freq_hi_hz: 136_000.0,
    description: "West and Central Africa forests. CF ~127 kHz. Often sympatric with H. caffer; distinguishable by lower frequency. Cryptic species pair.",
    echolocates: true,
};

pub const HIPPOSIDEROS_VITTATUS: BatSpecies = BatSpecies {
    id: "hipposideros_vittatus",
    name: "Striped Roundleaf Bat",
    scientific_name: "Hipposideros vittatus",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 56_000.0,
    freq_hi_hz: 66_000.0,
    description: "East and southern Africa. Large species; low CF ~60 kHz for a hipposiderid. Caves and large rock overhangs.",
    echolocates: true,
};

pub const HIPPOSIDEROS_GIGAS: BatSpecies = BatSpecies {
    id: "hipposideros_gigas",
    name: "Giant Roundleaf Bat",
    scientific_name: "Hipposideros gigas",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 56_000.0,
    freq_hi_hz: 66_000.0,
    description: "West and Central Africa forests. Largest hipposiderid; CF ~60\u{2013}63 kHz. Caves near forest.",
    echolocates: true,
};

pub const CLOEOTIS_PERCIVALI: BatSpecies = BatSpecies {
    id: "cloeotis_percivali",
    name: "Percival's Short-eared Trident Bat",
    scientific_name: "Cloeotis percivali",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 190_000.0,
    freq_hi_hz: 215_000.0,
    description: "Highest known bat echolocation frequency (~212 kHz). Southern and East Africa caves and mines. Tiny bat (3\u{2013}5 g). Requires >400 kHz sample rate detectors.",
    echolocates: true,
};

// ── Vespertilionidae ──

pub const SCOTOPHILUS_DINGANII: BatSpecies = BatSpecies {
    id: "scotophilus_dinganii",
    name: "Yellow-bellied House Bat",
    scientific_name: "Scotophilus dinganii",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 58_000.0,
    description: "Most common vespertilionid in sub-Saharan Africa. Two cryptic forms: ~33 kHz and ~44 kHz peak. Roosts in buildings. Hockey-stick call shape.",
    echolocates: true,
};

pub const SCOTOPHILUS_VIRIDIS: BatSpecies = BatSpecies {
    id: "scotophilus_viridis",
    name: "Green House Bat",
    scientific_name: "Scotophilus viridis",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 55_000.0,
    description: "East and southern African savannas. Peak ~40\u{2013}47 kHz. Smaller than S. dinganii. Roosts in buildings and tree hollows.",
    echolocates: true,
};

pub const SCOTOPHILUS_LEUCOGASTER: BatSpecies = BatSpecies {
    id: "scotophilus_leucogaster",
    name: "White-bellied House Bat",
    scientific_name: "Scotophilus leucogaster",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 45_000.0,
    description: "Sahel and West Africa savannas. Peak ~32\u{2013}35 kHz. Larger than S. dinganii. Roosts in buildings and palm trees.",
    echolocates: true,
};

pub const NEOROMICIA_CAPENSIS: BatSpecies = BatSpecies {
    id: "neoromicia_capensis",
    name: "Cape Serotine",
    scientific_name: "Neoromicia capensis",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 55_000.0,
    description: "Ubiquitous across southern Africa. Peak ~38 kHz. Hockey-stick FM-QCF calls. Roosts in buildings. One of the most frequently recorded species.",
    echolocates: true,
};

pub const PIPISTRELLUS_HESPERIDUS: BatSpecies = BatSpecies {
    id: "pipistrellus_hesperidus",
    name: "Dusky Pipistrelle",
    scientific_name: "Pipistrellus hesperidus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 65_000.0,
    description: "Sub-Saharan Africa. Peak ~45\u{2013}48 kHz. Small bat frequently detected around human habitation and streetlights.",
    echolocates: true,
};

pub const AFRONYCTERIS_NANUS: BatSpecies = BatSpecies {
    id: "afronycteris_nanus",
    name: "Banana Bat",
    scientific_name: "Afronycteris nanus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 60_000.0,
    description: "Sub-Saharan Africa. Peak ~43 kHz. FM calls lasting 4\u{2013}5 ms. Named for roosting in furled banana leaves. Small (3\u{2013}5 g).",
    echolocates: true,
};

pub const PIPISTRELLUS_RUEPPELLII: BatSpecies = BatSpecies {
    id: "pipistrellus_rueppellii",
    name: "R\u{fc}ppell's Pipistrelle",
    scientific_name: "Pipistrellus rueppellii",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 60_000.0,
    description: "North Africa, Sahel, East Africa, Middle East. Peak ~40\u{2013}44 kHz. Associated with arid habitats and waterways.",
    echolocates: true,
};

pub const MYOTIS_TRICOLOR: BatSpecies = BatSpecies {
    id: "myotis_tricolor",
    name: "Temminck's Myotis",
    scientific_name: "Myotis tricolor",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 80_000.0,
    description: "Southern and East Africa. Broadband FM sweep. Peak ~48 kHz. Caves and mines. Distinctive tricolored fur.",
    echolocates: true,
};

pub const MYOTIS_WELWITSCHII: BatSpecies = BatSpecies {
    id: "myotis_welwitschii",
    name: "Welwitsch's Myotis",
    scientific_name: "Myotis welwitschii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 55_000.0,
    description: "Sub-Saharan Africa woodlands. Lower peak ~34 kHz than M. tricolor. Short broadband FM calls. Roosts in foliage.",
    echolocates: true,
};

pub const EPTESICUS_HOTTENTOTUS: BatSpecies = BatSpecies {
    id: "eptesicus_hottentotus",
    name: "Long-tailed House Bat",
    scientific_name: "Eptesicus hottentotus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 50_000.0,
    description: "Southern Africa rocky areas. Peak ~30\u{2013}35 kHz. Distinctive long free tail. Roosts in rock crevices.",
    echolocates: true,
};

pub const LAEPHOTIS_BOTSWANAE: BatSpecies = BatSpecies {
    id: "laephotis_botswanae",
    name: "Botswana Long-eared Bat",
    scientific_name: "Laephotis botswanae",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 52_000.0,
    description: "Central and southern Africa. Peak ~37 kHz. Broadband FM. Large ears suggest some gleaning behavior. Poorly known species.",
    echolocates: true,
};

pub const GLAUCONYCTERIS_VARIEGATA: BatSpecies = BatSpecies {
    id: "glauconycteris_variegata",
    name: "Variegated Butterfly Bat",
    scientific_name: "Glauconycteris variegata",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 70_000.0,
    description: "Sub-Saharan Africa savanna and woodland. Broadband FM sweeps 70\u{2013}30 kHz. Distinctive wing markings. Slow fluttery flight.",
    echolocates: true,
};

pub const KERIVOULA_ARGENTATA: BatSpecies = BatSpecies {
    id: "kerivoula_argentata",
    name: "Damara Woolly Bat",
    scientific_name: "Kerivoula argentata",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 118_000.0,
    description: "East and southern Africa. Very high frequency, low-intensity FM calls (~90\u{2013}118 kHz). Clutter specialist in dense vegetation. Difficult to detect.",
    echolocates: true,
};

pub const KERIVOULA_LANOSA: BatSpecies = BatSpecies {
    id: "kerivoula_lanosa",
    name: "Lesser Woolly Bat",
    scientific_name: "Kerivoula lanosa",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 120_000.0,
    description: "Sub-Saharan Africa forests. Very high frequency broadband FM. Forest interior specialist. Similar to K. argentata but slightly different peak.",
    echolocates: true,
};

pub const NYCTICEINOPS_SCHLIEFFENI: BatSpecies = BatSpecies {
    id: "nycticeinops_schlieffeni",
    name: "Schlieffen's Twilight Bat",
    scientific_name: "Nycticeinops schlieffeni",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 60_000.0,
    description: "Sub-Saharan Africa savannas. Peak ~42 kHz. Often one of the first bats to emerge at dusk. Roosts in buildings and tree bark.",
    echolocates: true,
};

// ── Molossidae ──

pub const TADARIDA_AEGYPTIACA: BatSpecies = BatSpecies {
    id: "tadarida_aegyptiaca",
    name: "Egyptian Free-tailed Bat",
    scientific_name: "Tadarida aegyptiaca",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 12_000.0,
    freq_hi_hz: 28_000.0,
    description: "Throughout Africa and into the Middle East. Long narrowband QCF calls, peak ~18\u{2013}23 kHz. High-flying open-air forager. Very loud calls detectable at long range.",
    echolocates: true,
};

pub const CHAEREPHON_PUMILUS: BatSpecies = BatSpecies {
    id: "chaerephon_pumilus",
    name: "Little Free-tailed Bat",
    scientific_name: "Chaerephon pumilus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 32_000.0,
    description: "Sub-Saharan Africa, extremely common. Peak ~25 kHz. Often roosts in roofs of buildings in large colonies. Geographic variation in call frequency.",
    echolocates: true,
};

pub const MOPS_CONDYLURUS: BatSpecies = BatSpecies {
    id: "mops_condylurus",
    name: "Angolan Free-tailed Bat",
    scientific_name: "Mops condylurus",
    family: "Molossidae",
    call_type: "FM-QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 35_000.0,
    description: "Sub-Saharan Africa savannas. Peak ~25\u{2013}28 kHz. Roosts in buildings and tree hollows. Often in mixed colonies with Chaerephon pumilus.",
    echolocates: true,
};

pub const MOPS_MIDAS: BatSpecies = BatSpecies {
    id: "mops_midas",
    name: "Midas Free-tailed Bat",
    scientific_name: "Mops midas",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 22_000.0,
    description: "Sub-Saharan Africa. Very low frequency QCF, peak ~13\u{2013}16 kHz. Large molossid (40\u{2013}60 g). Calls partially audible to humans. High, fast flight.",
    echolocates: true,
};

pub const OTOMOPS_MARTIENSSENI: BatSpecies = BatSpecies {
    id: "otomops_martiensseni",
    name: "Large-eared Free-tailed Bat",
    scientific_name: "Otomops martiensseni",
    family: "Molossidae",
    call_type: "FM-QCF",
    freq_lo_hz: 8_000.0,
    freq_hi_hz: 18_000.0,
    description: "East and southern Africa. Very low frequency calls (~11\u{2013}14 kHz), audible to humans. Very large molossid. Caves and buildings. Individual call signatures documented.",
    echolocates: true,
};

pub const SAUROMYS_PETROPHILUS: BatSpecies = BatSpecies {
    id: "sauromys_petrophilus",
    name: "Roberts's Flat-headed Bat",
    scientific_name: "Sauromys petrophilus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 38_000.0,
    description: "Southern Africa. Peak ~30 kHz. Narrow bandwidth QCF. Roosts in rock crevices. Flattened skull for squeezing into narrow cracks.",
    echolocates: true,
};

pub const CHAEREPHON_ANSORGEI: BatSpecies = BatSpecies {
    id: "chaerephon_ansorgei",
    name: "Ansorge's Free-tailed Bat",
    scientific_name: "Chaerephon ansorgei",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 38_000.0,
    description: "West, Central, and East Africa forests. Peak ~28\u{2013}32 kHz. Forest edge and canopy forager.",
    echolocates: true,
};

pub const TADARIDA_FULMINANS: BatSpecies = BatSpecies {
    id: "tadarida_fulminans",
    name: "Madagascan Large Free-tailed Bat",
    scientific_name: "Tadarida fulminans",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 12_000.0,
    freq_hi_hz: 25_000.0,
    description: "East Africa and Madagascar. Low frequency QCF, peak ~16\u{2013}18 kHz. Large species. Long narrow wings for fast open-air flight.",
    echolocates: true,
};

pub const CHAEREPHON_CHAPINI: BatSpecies = BatSpecies {
    id: "chaerephon_chapini",
    name: "Chapin's Free-tailed Bat",
    scientific_name: "Chaerephon chapini",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 35_000.0,
    description: "Central and East Africa. Peak ~25\u{2013}28 kHz. Forest and forest edge. Similar to C. pumilus but forest-associated.",
    echolocates: true,
};

// ── Emballonuridae ──

pub const TAPHOZOUS_MAURITIANUS: BatSpecies = BatSpecies {
    id: "taphozous_mauritianus",
    name: "Mauritian Tomb Bat",
    scientific_name: "Taphozous mauritianus",
    family: "Emballonuridae",
    call_type: "CF-FM",
    freq_lo_hz: 11_000.0,
    freq_hi_hz: 30_000.0,
    description: "Sub-Saharan Africa and Madagascar. Multiharmonic CF search calls with FM approach calls. Fundamental at 11\u{2013}13 kHz. Roosts on walls and tree trunks.",
    echolocates: true,
};

pub const TAPHOZOUS_PERFORATUS: BatSpecies = BatSpecies {
    id: "taphozous_perforatus",
    name: "Egyptian Tomb Bat",
    scientific_name: "Taphozous perforatus",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 35_000.0,
    description: "North and East Africa, arid regions. QCF calls peaking ~25\u{2013}28 kHz. Roosts in tombs, ruins, rock faces.",
    echolocates: true,
};

pub const COLEURA_AFRA: BatSpecies = BatSpecies {
    id: "coleura_afra",
    name: "African Sheath-tailed Bat",
    scientific_name: "Coleura afra",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 40_000.0,
    description: "East Africa coast, caves and rock shelters. Low-duty-cycle QCF calls peaking at ~33 kHz. Near Threatened. Colonial in coastal caves.",
    echolocates: true,
};

// ── Nycteridae ──

pub const NYCTERIS_THEBAICA: BatSpecies = BatSpecies {
    id: "nycteris_thebaica",
    name: "Egyptian Slit-faced Bat",
    scientific_name: "Nycteris thebaica",
    family: "Nycteridae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 120_000.0,
    description: "Most widespread African nycterid. Very low intensity broadband FM calls (\u{201c}whispering bat\u{201d}). Gleaner; uses passive listening. Very difficult to detect on bat detectors.",
    echolocates: true,
};

pub const NYCTERIS_GRANDIS: BatSpecies = BatSpecies {
    id: "nycteris_grandis",
    name: "Large Slit-faced Bat",
    scientific_name: "Nycteris grandis",
    family: "Nycteridae",
    call_type: "FM",
    freq_lo_hz: 17_000.0,
    freq_hi_hz: 114_000.0,
    description: "Central and West Africa forests. Largest nycterid. Very low intensity broadband FM. Hunts vertebrate prey (fish, frogs, smaller bats).",
    echolocates: true,
};

pub const NYCTERIS_MACROTIS: BatSpecies = BatSpecies {
    id: "nycteris_macrotis",
    name: "Large-eared Slit-faced Bat",
    scientific_name: "Nycteris macrotis",
    family: "Nycteridae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 110_000.0,
    description: "West and Central Africa forests. Low-intensity broadband FM similar to N. thebaica but larger. Gleaner in forest understory.",
    echolocates: true,
};

// ── Miniopteridae ──

pub const MINIOPTERUS_NATALENSIS: BatSpecies = BatSpecies {
    id: "miniopterus_natalensis",
    name: "Natal Long-fingered Bat",
    scientific_name: "Miniopterus natalensis",
    family: "Miniopteridae",
    call_type: "FM-QCF",
    freq_lo_hz: 52_000.0,
    freq_hi_hz: 85_000.0,
    description: "Southern and East Africa. Peak ~59 kHz. Large cave colonies (thousands). FM-QCF hockey-stick calls. Key cave-roosting species.",
    echolocates: true,
};

pub const MINIOPTERUS_FRATERCULUS: BatSpecies = BatSpecies {
    id: "miniopterus_fraterculus",
    name: "Lesser Long-fingered Bat",
    scientific_name: "Miniopterus fraterculus",
    family: "Miniopteridae",
    call_type: "FM-QCF",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 95_000.0,
    description: "Southern Africa. Peak ~71 kHz (12 kHz higher than M. natalensis). Smaller species. Cave-roosting.",
    echolocates: true,
};

pub const MINIOPTERUS_INFLATUS: BatSpecies = BatSpecies {
    id: "miniopterus_inflatus",
    name: "Greater Long-fingered Bat",
    scientific_name: "Miniopterus inflatus",
    family: "Miniopteridae",
    call_type: "FM-QCF",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 75_000.0,
    description: "Central and East Africa. Larger species; lower peak ~52\u{2013}55 kHz than M. natalensis. Caves in forested areas.",
    echolocates: true,
};

// ── Pteropodidae (non-echolocating, except Rousettus) ──

pub const ROUSETTUS_AEGYPTIACUS: BatSpecies = BatSpecies {
    id: "rousettus_aegyptiacus",
    name: "Egyptian Rousette",
    scientific_name: "Rousettus aegyptiacus",
    family: "Pteropodidae",
    call_type: "clicks",
    freq_lo_hz: 12_000.0,
    freq_hi_hz: 70_000.0,
    description: "Only African fruit bat with true echolocation (tongue clicks). Broadband clicks ~50\u{2013}100 \u{b5}s duration. Caves and mines. Very large colonies.",
    echolocates: true,
};

pub const EIDOLON_HELVUM: BatSpecies = BatSpecies {
    id: "eidolon_helvum",
    name: "Straw-coloured Fruit Bat",
    scientific_name: "Eidolon helvum",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Most widespread African megabat. Massive colonies (up to 1 million). Important seed disperser. Uses vision and smell only.",
    echolocates: false,
};

pub const EPOMOPHORUS_WAHLBERGI: BatSpecies = BatSpecies {
    id: "epomophorus_wahlbergi",
    name: "Wahlberg's Epauletted Fruit Bat",
    scientific_name: "Epomophorus wahlbergi",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "East and southern Africa savannas. Males produce loud honking display calls (audible). No echolocation. Common in gardens.",
    echolocates: false,
};

pub const HYPSIGNATHUS_MONSTROSUS: BatSpecies = BatSpecies {
    id: "hypsignathus_monstrosus",
    name: "Hammer-headed Fruit Bat",
    scientific_name: "Hypsignathus monstrosus",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Central and West Africa forests. Largest African bat (wingspan to 90 cm). Males have enlarged larynx for loud lek-display calls. No echolocation.",
    echolocates: false,
};

pub const EPOMOPS_FRANQUETI: BatSpecies = BatSpecies {
    id: "epomops_franqueti",
    name: "Franquet's Epauletted Fruit Bat",
    scientific_name: "Epomops franqueti",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "Central and West Africa tropical forests. Males produce repetitive metallic calls during display. Important pollinator and seed disperser.",
    echolocates: false,
};

// ══════════════════════════════════════════════════════════════════════════════
// Asia & Middle East — species used across Southeast Asia, South Asia,
// East Asia, and Middle East bat books
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Francis (2008) A Field Guide to the Mammals of South-east Asia
// - Kingston et al. (2006) Acoustic diversity of SE Asian bats; Bioacoustics
// - Bates & Harrison (1997) Bats of the Indian Subcontinent
// - Wordley et al. (2014) Acoustic ID of bats in southern Western Ghats; Acta Chiropt.
// - Shi et al. (2009) Echolocation calls of Chinese bats; Mammalia
// - Benda et al. (2006, 2012) Bats of the eastern Mediterranean and Middle East
// - Korine & Pinshow (2004) Guild structure in a Negev desert bat community
// - Struebig et al. (2006) Borneo bat community acoustics

// ── Rhinolophidae ──

pub const RHINOLOPHUS_AFFINIS: BatSpecies = BatSpecies {
    id: "rhinolophus_affinis",
    name: "Intermediate Horseshoe Bat",
    scientific_name: "Rhinolophus affinis",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 68_000.0,
    freq_hi_hz: 90_000.0,
    description: "CF ~74\u{2013}87 kHz (varies geographically). One of the most widespread horseshoe bats from South Asia through China and SE Asia. Common in forests, caves, and forest edges.",
    echolocates: true,
};

pub const RHINOLOPHUS_LUCTUS: BatSpecies = BatSpecies {
    id: "rhinolophus_luctus",
    name: "Woolly Horseshoe Bat",
    scientific_name: "Rhinolophus luctus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 27_000.0,
    freq_hi_hz: 40_000.0,
    description: "Largest horseshoe bat in Asia (~35 g). Unusually low CF ~28\u{2013}35 kHz due to large body size. Dense woolly fur. Primary forests from Nepal through China to Borneo.",
    echolocates: true,
};

pub const RHINOLOPHUS_PUSILLUS: BatSpecies = BatSpecies {
    id: "rhinolophus_pusillus",
    name: "Least Horseshoe Bat",
    scientific_name: "Rhinolophus pusillus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 120_000.0,
    description: "Very small horseshoe bat (~3 g). High CF ~103\u{2013}115 kHz. Widespread across southern China, Taiwan, and SE Asia in forest and scrub habitats.",
    echolocates: true,
};

pub const RHINOLOPHUS_MALAYANUS: BatSpecies = BatSpecies {
    id: "rhinolophus_malayanus",
    name: "Malayan Horseshoe Bat",
    scientific_name: "Rhinolophus malayanus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 90_000.0,
    freq_hi_hz: 105_000.0,
    description: "CF ~95\u{2013}100 kHz. Widespread in mainland SE Asia (Thailand, Vietnam, Myanmar, Laos, Cambodia). Roosts in caves and tunnels; forages in and near forest.",
    echolocates: true,
};

pub const RHINOLOPHUS_BORNEENSIS: BatSpecies = BatSpecies {
    id: "rhinolophus_borneensis",
    name: "Bornean Horseshoe Bat",
    scientific_name: "Rhinolophus borneensis",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 72_000.0,
    freq_hi_hz: 86_000.0,
    description: "CF ~80\u{2013}83 kHz. Endemic to Borneo and Palawan (Philippines). Similar frequency to R. affinis; distinguished by morphology. Forest-dependent.",
    echolocates: true,
};

pub const RHINOLOPHUS_TRIFOLIATUS: BatSpecies = BatSpecies {
    id: "rhinolophus_trifoliatus",
    name: "Trefoil Horseshoe Bat",
    scientific_name: "Rhinolophus trifoliatus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 70_000.0,
    description: "Distinctive three-leafed noseleaf. CF ~60\u{2013}64 kHz. Sundaic lowland forest (Malaysia, Indonesia, Brunei). Found in both primary and disturbed forest.",
    echolocates: true,
};

pub const RHINOLOPHUS_ACUMINATUS: BatSpecies = BatSpecies {
    id: "rhinolophus_acuminatus",
    name: "Acuminate Horseshoe Bat",
    scientific_name: "Rhinolophus acuminatus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 58_000.0,
    freq_hi_hz: 74_000.0,
    description: "CF ~63\u{2013}68 kHz. Philippines and Sunda shelf islands (Indonesia). Forest interior species. Closely related to R. stheno.",
    echolocates: true,
};

pub const RHINOLOPHUS_PEARSONII: BatSpecies = BatSpecies {
    id: "rhinolophus_pearsonii",
    name: "Pearson's Horseshoe Bat",
    scientific_name: "Rhinolophus pearsonii",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 72_000.0,
    description: "CF ~58\u{2013}70 kHz (varies across range). Hilly and montane forests of mainland SE Asia and south China. Distinctive large ears. Often found alongside R. affinis.",
    echolocates: true,
};

pub const RHINOLOPHUS_ROUXII: BatSpecies = BatSpecies {
    id: "rhinolophus_rouxii",
    name: "Rufous Horseshoe Bat",
    scientific_name: "Rhinolophus rouxii",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 74_000.0,
    freq_hi_hz: 86_000.0,
    description: "CF ~78\u{2013}85 kHz; higher in south Indian populations. Widespread across the Indian subcontinent and Sri Lanka. Common in forests and caves.",
    echolocates: true,
};

pub const RHINOLOPHUS_INDOROUXII: BatSpecies = BatSpecies {
    id: "rhinolophus_indorouxii",
    name: "Indian Rufous Horseshoe Bat",
    scientific_name: "Rhinolophus indorouxii",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 87_000.0,
    freq_hi_hz: 95_000.0,
    description: "CF ~87\u{2013}95 kHz (mean ~92 kHz). Recently split from R. rouxii; endemic to the southern Western Ghats and Sri Lanka. Caves and tunnels in humid forests.",
    echolocates: true,
};

pub const RHINOLOPHUS_LEPIDUS: BatSpecies = BatSpecies {
    id: "rhinolophus_lepidus",
    name: "Blyth's Horseshoe Bat",
    scientific_name: "Rhinolophus lepidus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 112_000.0,
    description: "CF ~104\u{2013}110 kHz. One of the most widespread horseshoe bats in South Asia, from Pakistan through India to Nepal. Caves, rock crevices, and buildings.",
    echolocates: true,
};

pub const RHINOLOPHUS_BEDDOMEI: BatSpecies = BatSpecies {
    id: "rhinolophus_beddomei",
    name: "Lesser Woolly Horseshoe Bat",
    scientific_name: "Rhinolophus beddomei",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 46_000.0,
    freq_hi_hz: 52_000.0,
    description: "CF ~47\u{2013}50 kHz. Endemic to peninsular India and Sri Lanka; Western and Eastern Ghats moist forests. Cave-roosting.",
    echolocates: true,
};

pub const RHINOLOPHUS_SINICUS: BatSpecies = BatSpecies {
    id: "rhinolophus_sinicus",
    name: "Chinese Rufous Horseshoe Bat",
    scientific_name: "Rhinolophus sinicus",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 73_000.0,
    freq_hi_hz: 89_000.0,
    description: "CF ~73\u{2013}89 kHz (mean ~82\u{2013}85 kHz; females higher). Widespread in southern and central China. Cave-roosting, often sympatric with R. affinis. Important SARS-CoV host species.",
    echolocates: true,
};

pub const RHINOLOPHUS_MACROTIS: BatSpecies = BatSpecies {
    id: "rhinolophus_macrotis",
    name: "Big-eared Horseshoe Bat",
    scientific_name: "Rhinolophus macrotis",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 42_000.0,
    freq_hi_hz: 66_000.0,
    description: "CF ~42\u{2013}58 kHz (large and small cryptic forms). Distinctive very large ears. Widespread in southern and central China in karst terrain. May represent distinct species.",
    echolocates: true,
};

pub const RHINOLOPHUS_MONOCEROS: BatSpecies = BatSpecies {
    id: "rhinolophus_monoceros",
    name: "Formosan Lesser Horseshoe Bat",
    scientific_name: "Rhinolophus monoceros",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 107_000.0,
    freq_hi_hz: 118_000.0,
    description: "CF ~108\u{2013}117 kHz. Endemic to Taiwan. Geographic variation up to 6 kHz across populations. Cave-roosting; widespread in forested mountains.",
    echolocates: true,
};

pub const RHINOLOPHUS_MEHELYI: BatSpecies = BatSpecies {
    id: "rhinolophus_mehelyi",
    name: "Mehely's Horseshoe Bat",
    scientific_name: "Rhinolophus mehelyi",
    family: "Rhinolophidae",
    call_type: "CF",
    freq_lo_hz: 103_000.0,
    freq_hi_hz: 113_000.0,
    description: "CF ~107\u{2013}111 kHz; closely overlaps R. euryale in frequency. Mediterranean and Middle East (Turkey, Israel, Jordan, Iran). Cave-roosting, often with R. euryale and R. blasii.",
    echolocates: true,
};

// ── Hipposideridae ──

pub const HIPPOSIDEROS_ARMIGER: BatSpecies = BatSpecies {
    id: "hipposideros_armiger",
    name: "Great Roundleaf Bat",
    scientific_name: "Hipposideros armiger",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 55_000.0,
    freq_hi_hz: 80_000.0,
    description: "Largest Hipposideros in Asia (~35 g). CF ~60\u{2013}75 kHz. Widespread from India through China to SE Asia. Cave-roosting, often in large colonies. Doppler-shift compensation.",
    echolocates: true,
};

pub const HIPPOSIDEROS_LARVATUS: BatSpecies = BatSpecies {
    id: "hipposideros_larvatus",
    name: "Intermediate Roundleaf Bat",
    scientific_name: "Hipposideros larvatus",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 68_000.0,
    freq_hi_hz: 102_000.0,
    description: "Very common medium-sized roundleaf bat. CF ~77\u{2013}98 kHz (cryptic species complex with two phonic types). Throughout SE Asia and southern China. Caves, gardens, and forest.",
    echolocates: true,
};

pub const HIPPOSIDEROS_BICOLOR: BatSpecies = BatSpecies {
    id: "hipposideros_bicolor",
    name: "Bicolored Roundleaf Bat",
    scientific_name: "Hipposideros bicolor",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 96_000.0,
    freq_hi_hz: 161_000.0,
    description: "Small roundleaf bat. High CF varies greatly by region (~106\u{2013}110 kHz in SE Asia, ~147\u{2013}161 kHz in South Asia). Species complex. Extremely widespread from India to the Philippines. Caves and forest edge.",
    echolocates: true,
};

pub const HIPPOSIDEROS_GALERITUS: BatSpecies = BatSpecies {
    id: "hipposideros_galeritus",
    name: "Cantor's Roundleaf Bat",
    scientific_name: "Hipposideros galeritus",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 88_000.0,
    freq_hi_hz: 106_000.0,
    description: "CF ~94\u{2013}100 kHz. Sundaland (Malaysia, Indonesia, Borneo) in lowland dipterocarp forest. Small caves and rock shelters.",
    echolocates: true,
};

pub const HIPPOSIDEROS_DIADEMA: BatSpecies = BatSpecies {
    id: "hipposideros_diadema",
    name: "Diadem Roundleaf Bat",
    scientific_name: "Hipposideros diadema",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 52_000.0,
    freq_hi_hz: 68_000.0,
    description: "Large roundleaf bat with striking pale shoulder markings. CF ~58\u{2013}63 kHz. Mainland SE Asia to Indonesia and the Philippines. Large cave colonies; long-distance forager.",
    echolocates: true,
};

pub const HIPPOSIDEROS_POMONA: BatSpecies = BatSpecies {
    id: "hipposideros_pomona",
    name: "Pomona Roundleaf Bat",
    scientific_name: "Hipposideros pomona",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 134_000.0,
    freq_hi_hz: 160_000.0,
    description: "Very high CF ~140\u{2013}156 kHz, highest of any mainland SE Asian hipposiderid. Vietnam, Thailand, Laos, Myanmar, and India. Small cave groups in dense forest.",
    echolocates: true,
};

pub const COELOPS_FRITHII: BatSpecies = BatSpecies {
    id: "coelops_frithii",
    name: "Tail-less Roundleaf Bat",
    scientific_name: "Coelops frithii",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 130_000.0,
    description: "Tiny tail-less roundleaf bat. CF ~110\u{2013}120 kHz. Mainland SE Asia and Sundaland. Unusual for foraging in open grass habitats rather than forest interior.",
    echolocates: true,
};

pub const HIPPOSIDEROS_SPEORIS: BatSpecies = BatSpecies {
    id: "hipposideros_speoris",
    name: "Schneider's Leaf-nosed Bat",
    scientific_name: "Hipposideros speoris",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 127_000.0,
    freq_hi_hz: 138_000.0,
    description: "CF ~127\u{2013}138 kHz. One of the most studied South Asian hipposiderids. Widespread across peninsular India and Sri Lanka; caves, temples, and old buildings.",
    echolocates: true,
};

pub const HIPPOSIDEROS_LANKADIVA: BatSpecies = BatSpecies {
    id: "hipposideros_lankadiva",
    name: "Kolar Leaf-nosed Bat",
    scientific_name: "Hipposideros lankadiva",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 60_000.0,
    freq_hi_hz: 70_000.0,
    description: "CF ~65\u{2013}70 kHz; unusually low for a hipposiderid due to large body size. Endemic to peninsular India and Sri Lanka. Caves and old mines. IUCN Near Threatened.",
    echolocates: true,
};

pub const HIPPOSIDEROS_DURGADASI: BatSpecies = BatSpecies {
    id: "hipposideros_durgadasi",
    name: "Durgadas's Leaf-nosed Bat",
    scientific_name: "Hipposideros durgadasi",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 163_000.0,
    freq_hi_hz: 178_000.0,
    description: "CF ~163\u{2013}176 kHz; among the highest-frequency bats in South Asia. Endemic to peninsular India cave systems. Poorly studied; IUCN Vulnerable.",
    echolocates: true,
};

pub const HIPPOSIDEROS_PRATTI: BatSpecies = BatSpecies {
    id: "hipposideros_pratti",
    name: "Pratt's Leaf-nosed Bat",
    scientific_name: "Hipposideros pratti",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 52_000.0,
    freq_hi_hz: 70_000.0,
    description: "CF 2nd harmonic ~59\u{2013}60 kHz; 1st harmonic ~29 kHz. Large species endemic to China. Cave-roosting in central and southern China (Hubei, Sichuan, Guizhou). Well-studied for auditory neuroscience.",
    echolocates: true,
};

pub const ASELLISCUS_STOLICZKANUS: BatSpecies = BatSpecies {
    id: "aselliscus_stoliczkanus",
    name: "Stoliczka's Trident Bat",
    scientific_name: "Aselliscus stoliczkanus",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 125_000.0,
    description: "CF ~119\u{2013}120 kHz with terminal FM sweep. Distinctive trident nose-leaf. Southeast China (Yunnan, Guizhou, Guangxi) in karst limestone. Cave-roosting.",
    echolocates: true,
};

pub const ASELLIA_TRIDENS: BatSpecies = BatSpecies {
    id: "asellia_tridens",
    name: "Trident Leaf-nosed Bat",
    scientific_name: "Asellia tridens",
    family: "Hipposideridae",
    call_type: "CF",
    freq_lo_hz: 96_000.0,
    freq_hi_hz: 108_000.0,
    description: "CF ~98\u{2013}104 kHz. Most widespread hipposiderid in the Middle East and North Africa. Trident-shaped noseleaf. Highly colonial; desert-adapted. Morocco to Pakistan.",
    echolocates: true,
};

// ── Rhinopomatidae ──

pub const RHINOPOMA_MICROPHYLLUM: BatSpecies = BatSpecies {
    id: "rhinopoma_microphyllum",
    name: "Greater Mouse-tailed Bat",
    scientific_name: "Rhinopoma microphyllum",
    family: "Rhinopomatidae",
    call_type: "FM-QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 40_000.0,
    description: "QCF search phase ~20\u{2013}22 kHz with short FM onset. The largest Rhinopoma. Widespread from Morocco to South Asia. Desert-adapted; roosts in ruins, caves, and rock crevices. Family Rhinopomatidae is endemic to the Old World arid zone.",
    echolocates: true,
};

pub const RHINOPOMA_HARDWICKII: BatSpecies = BatSpecies {
    id: "rhinopoma_hardwickii",
    name: "Lesser Mouse-tailed Bat",
    scientific_name: "Rhinopoma hardwickii",
    family: "Rhinopomatidae",
    call_type: "FM-QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 50_000.0,
    description: "Search-phase QCF ~22\u{2013}28 kHz with FM onset. Widespread in arid zones from North Africa through Iran and Pakistan to India. Often syntopic with R. microphyllum; separated by smaller size and higher frequency.",
    echolocates: true,
};

pub const RHINOPOMA_CYSTOPS: BatSpecies = BatSpecies {
    id: "rhinopoma_cystops",
    name: "Small Mouse-tailed Bat",
    scientific_name: "Rhinopoma cystops",
    family: "Rhinopomatidae",
    call_type: "FM-QCF",
    freq_lo_hz: 28_000.0,
    freq_hi_hz: 55_000.0,
    description: "Smallest Rhinopoma; higher frequency than congeners (peak ~35\u{2013}40 kHz). Northeast Africa (Egypt, Sudan) and the Levant (Israel, Jordan, Syria). Rocky desert habitats.",
    echolocates: true,
};

// ── Megadermatidae ──

pub const MEGADERMA_LYRA: BatSpecies = BatSpecies {
    id: "megaderma_lyra",
    name: "Greater False Vampire Bat",
    scientific_name: "Megaderma lyra",
    family: "Megadermatidae",
    call_type: "FM",
    freq_lo_hz: 19_000.0,
    freq_hi_hz: 100_000.0,
    description: "Low-intensity multiharmonic FM calls (~19\u{2013}100 kHz). Large carnivorous bat preying on frogs, lizards, and other bats. Locates prey mainly by passive listening. South and mainland SE Asia; caves, ruins, and culverts.",
    echolocates: true,
};

pub const MEGADERMA_SPASMA: BatSpecies = BatSpecies {
    id: "megaderma_spasma",
    name: "Lesser False Vampire Bat",
    scientific_name: "Megaderma spasma",
    family: "Megadermatidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 120_000.0,
    description: "Low-intensity broadband FM calls. Smaller than M. lyra. Primarily insectivorous. Mainland SE Asia through Sundaland to the Philippines. Caves, hollow trees, and buildings.",
    echolocates: true,
};

// ── Emballonuridae ──

pub const TAPHOZOUS_LONGIMANUS: BatSpecies = BatSpecies {
    id: "taphozous_longimanus",
    name: "Long-winged Tomb Bat",
    scientific_name: "Taphozous longimanus",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 24_000.0,
    freq_hi_hz: 45_000.0,
    description: "QCF calls ~28\u{2013}35 kHz. Fast direct flight in open areas. South and SE Asia from India to Indonesia. Roosts on exposed surfaces of buildings and rock faces.",
    echolocates: true,
};

pub const TAPHOZOUS_THEOBALDI: BatSpecies = BatSpecies {
    id: "taphozous_theobaldi",
    name: "Theobald's Tomb Bat",
    scientific_name: "Taphozous theobaldi",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 35_000.0,
    description: "Large tomb bat. QCF ~25\u{2013}30 kHz. Mainland SE Asia (Myanmar, Thailand, Vietnam, Cambodia). Obligate cave-roosting, often in large limestone cave colonies.",
    echolocates: true,
};

pub const EMBALLONURA_MONTICOLA: BatSpecies = BatSpecies {
    id: "emballonura_monticola",
    name: "Lesser Sheath-tailed Bat",
    scientific_name: "Emballonura monticola",
    family: "Emballonuridae",
    call_type: "FM-QCF",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 80_000.0,
    description: "FM-QCF calls ~55\u{2013}65 kHz. Small sheath-tailed bat. Lowland forest of Sundaland (Malaysia, Indonesia, Brunei). Small groups on cave walls; forest interior forager.",
    echolocates: true,
};

pub const TAPHOZOUS_NUDIVENTRIS: BatSpecies = BatSpecies {
    id: "taphozous_nudiventris",
    name: "Naked-rumped Tomb Bat",
    scientific_name: "Taphozous nudiventris",
    family: "Emballonuridae",
    call_type: "QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 40_000.0,
    description: "QCF ~25\u{2013}30 kHz. Bare rump patch. Most commonly encountered tomb bat in the Middle East. Egypt, Levant, Iraq, Iran, Saudi Arabia, Yemen. Ruins, tombs, and rock fissures; often large colonies.",
    echolocates: true,
};

// ── Molossidae ──

pub const CHAEREPHON_PLICATUS: BatSpecies = BatSpecies {
    id: "chaerephon_plicatus",
    name: "Wrinkle-lipped Free-tailed Bat",
    scientific_name: "Chaerephon plicatus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 13_000.0,
    freq_hi_hz: 32_000.0,
    description: "Narrowband QCF ~16\u{2013}30 kHz. Forms some of the world\u{2019}s largest bat colonies (millions). Fast high-altitude open-air forager. Widespread across South and SE Asia. Spectacular synchronous dusk emergence columns.",
    echolocates: true,
};

pub const OTOMOPS_FORMOSUS: BatSpecies = BatSpecies {
    id: "otomops_formosus",
    name: "Javan Mastiff Bat",
    scientific_name: "Otomops formosus",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 18_000.0,
    description: "Very large molossid with long ears. Very low QCF ~12\u{2013}16 kHz. Rare; primarily Java and Borneo. High-altitude open-air forager.",
    echolocates: true,
};

pub const MOPS_MOPS: BatSpecies = BatSpecies {
    id: "mops_mops",
    name: "Malayan Free-tailed Bat",
    scientific_name: "Mops mops",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 30_000.0,
    description: "QCF ~22\u{2013}28 kHz. Sundaland (Malaysia, Indonesia, Brunei, Singapore). Hollow trees and buildings. Forages high above canopy.",
    echolocates: true,
};

pub const OTOMOPS_WROUGHTONI: BatSpecies = BatSpecies {
    id: "otomops_wroughtoni",
    name: "Wroughton's Free-tailed Bat",
    scientific_name: "Otomops wroughtoni",
    family: "Molossidae",
    call_type: "QCF",
    freq_lo_hz: 12_000.0,
    freq_hi_hz: 18_000.0,
    description: "Very low frequency QCF ~14\u{2013}17 kHz; among the lowest of any bat. Rare; known from a few cave sites in peninsular India (Karnataka, Goa) and Meghalaya. IUCN Data Deficient.",
    echolocates: true,
};

// ── Miniopteridae ──

pub const MINIOPTERUS_MAGNATER: BatSpecies = BatSpecies {
    id: "miniopterus_magnater",
    name: "Greater Bent-wing Bat",
    scientific_name: "Miniopterus magnater",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 42_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM sweeps, terminal frequency ~44\u{2013}55 kHz. Largest Miniopterus in South and SE Asia. Large cave colonies. Fast agile flight. Previously misidentified as M. schreibersii.",
    echolocates: true,
};

pub const MINIOPTERUS_PUSILLUS: BatSpecies = BatSpecies {
    id: "miniopterus_pusillus",
    name: "Small Bent-wing Bat",
    scientific_name: "Miniopterus pusillus",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM calls, terminal frequency ~52\u{2013}58 kHz (higher than M. magnater). Widespread in SE Asia. Often co-roosts with M. magnater in large limestone caves.",
    echolocates: true,
};

pub const MINIOPTERUS_PALLIDUS: BatSpecies = BatSpecies {
    id: "miniopterus_pallidus",
    name: "Pale Bent-wing Bat",
    scientific_name: "Miniopterus pallidus",
    family: "Miniopteridae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 65_000.0,
    description: "FM ~47\u{2013}62 kHz, acoustically indistinguishable from M. schreibersii. Central Asian and Middle Eastern distribution (Kazakhstan, Uzbekistan, Iran, Afghanistan). Recently split from M. schreibersii. Large cave colonies; migratory.",
    echolocates: true,
};

// ── Vespertilionidae ──

pub const MYOTIS_HORSFIELDII: BatSpecies = BatSpecies {
    id: "myotis_horsfieldii",
    name: "Horsfield's Myotis",
    scientific_name: "Myotis horsfieldii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 100_000.0,
    description: "Broadband FM, peak ~45\u{2013}55 kHz. Trawling insectivore catching prey from water surfaces using large hind feet. Widespread across South and SE Asia in forested lowlands near streams.",
    echolocates: true,
};

pub const MYOTIS_MURICOLA: BatSpecies = BatSpecies {
    id: "myotis_muricola",
    name: "Wall-roosting Myotis",
    scientific_name: "Myotis muricola",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 115_000.0,
    description: "Broadband FM, peak ~65\u{2013}80 kHz. Very widespread and commonly recorded in SE Asia. Roosts on vertical surfaces (rock faces, buildings, dead fronds). Forest understory and edge forager.",
    echolocates: true,
};

pub const MYOTIS_HASSELTII: BatSpecies = BatSpecies {
    id: "myotis_hasseltii",
    name: "Lesser Large-footed Myotis",
    scientific_name: "Myotis hasseltii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM, peak ~50\u{2013}60 kHz. Large-footed trawling bat capturing prey from water surfaces. Sundaland and mainland SE Asia; almost always near still or slow-moving water.",
    echolocates: true,
};

pub const KERIVOULA_HARDWICKII: BatSpecies = BatSpecies {
    id: "kerivoula_hardwickii",
    name: "Hardwicke's Woolly Bat",
    scientific_name: "Kerivoula hardwickii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 100_000.0,
    freq_hi_hz: 160_000.0,
    description: "Very high-frequency FM, peak ~120\u{2013}140 kHz. Tiny orange-furred bat foraging in dense forest understory. One of the highest-frequency vespertilionids. Sundaland; often roosts in pitcher plants.",
    echolocates: true,
};

pub const KERIVOULA_PELLUCIDA: BatSpecies = BatSpecies {
    id: "kerivoula_pellucida",
    name: "Clear-winged Woolly Bat",
    scientific_name: "Kerivoula pellucida",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 90_000.0,
    freq_hi_hz: 145_000.0,
    description: "Very high-frequency FM, peak ~115\u{2013}130 kHz. Partially transparent wing membranes. Lowland forest of Sundaland. Gleans insects from foliage.",
    echolocates: true,
};

pub const KERIVOULA_PICTA: BatSpecies = BatSpecies {
    id: "kerivoula_picta",
    name: "Painted Bat",
    scientific_name: "Kerivoula picta",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 41_000.0,
    freq_hi_hz: 157_000.0,
    description: "Steep broadband FM ~157\u{2013}41 kHz; very low-intensity whisper echolocator. Striking orange-and-black wing membranes. South and SE Asian lowland forests. Roosts in dead bamboo and banana flowers.",
    echolocates: true,
};

pub const MURINA_CYCLOTIS: BatSpecies = BatSpecies {
    id: "murina_cyclotis",
    name: "Round-eared Tube-nosed Bat",
    scientific_name: "Murina cyclotis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 43_000.0,
    freq_hi_hz: 152_000.0,
    description: "Very broadband FM ~152\u{2013}43 kHz. Tubular nostrils; extreme-clutter specialist. Widespread across South and SE Asia in forests. Slow-flying gleaner; roosts in leaves and dead fronds.",
    echolocates: true,
};

pub const TYLONYCTERIS_PACHYPUS: BatSpecies = BatSpecies {
    id: "tylonycteris_pachypus",
    name: "Lesser Bamboo Bat",
    scientific_name: "Tylonycteris pachypus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 50_000.0,
    freq_hi_hz: 90_000.0,
    description: "FM, peak ~65\u{2013}75 kHz. One of the world\u{2019}s smallest bats (~2 g). Flattened skull and adhesive pads for roosting inside bamboo internodes. Widespread in bamboo forests across SE Asia.",
    echolocates: true,
};

pub const TYLONYCTERIS_ROBUSTULA: BatSpecies = BatSpecies {
    id: "tylonycteris_robustula",
    name: "Greater Bamboo Bat",
    scientific_name: "Tylonycteris robustula",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 82_000.0,
    description: "FM, peak ~55\u{2013}65 kHz (lower than T. pachypus). Same bamboo-roosting ecology. Widespread in SE Asian bamboo forests. Often co-roosts with T. pachypus.",
    echolocates: true,
};

pub const SCOTOPHILUS_KUHLII: BatSpecies = BatSpecies {
    id: "scotophilus_kuhlii",
    name: "Lesser Asiatic Yellow Bat",
    scientific_name: "Scotophilus kuhlii",
    family: "Vespertilionidae",
    call_type: "QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 58_000.0,
    description: "QCF, peak ~40\u{2013}47 kHz. One of the most commonly recorded bats in SE Asian urban and agricultural areas. Bright yellow-orange undersides. Buildings and palm crowns. Highly synanthropic.",
    echolocates: true,
};

pub const SCOTOPHILUS_HEATHII: BatSpecies = BatSpecies {
    id: "scotophilus_heathii",
    name: "Greater Asiatic Yellow Bat",
    scientific_name: "Scotophilus heathii",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 24_000.0,
    freq_hi_hz: 45_000.0,
    description: "FM-QCF, peak ~28\u{2013}32 kHz; low frequency for its size. One of the most common bats across the Indian subcontinent. Buildings, hollow trees, and roof spaces. Open and edge habitat forager.",
    echolocates: true,
};

pub const PIPISTRELLUS_JAVANICUS: BatSpecies = BatSpecies {
    id: "pipistrellus_javanicus",
    name: "Javan Pipistrelle",
    scientific_name: "Pipistrellus javanicus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 75_000.0,
    description: "FM-QCF, peak ~50\u{2013}55 kHz. Common urban pipistrelle. South Asia through mainland SE Asia to the Greater Sundas. Buildings; gardens, parks, and agricultural areas.",
    echolocates: true,
};

pub const PIPISTRELLUS_CEYLONICUS: BatSpecies = BatSpecies {
    id: "pipistrellus_ceylonicus",
    name: "Kelaart's Pipistrelle",
    scientific_name: "Pipistrellus ceylonicus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 30_000.0,
    freq_hi_hz: 76_000.0,
    description: "FM-QCF, peak ~36\u{2013}38 kHz. One of the most commonly detected bat species acoustically in South Asian surveys. Widespread across the Indian subcontinent and Sri Lanka. Water and forest edges.",
    echolocates: true,
};

pub const PIPISTRELLUS_COROMANDRA: BatSpecies = BatSpecies {
    id: "pipistrellus_coromandra",
    name: "Indian Pipistrelle",
    scientific_name: "Pipistrellus coromandra",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 36_000.0,
    freq_hi_hz: 70_000.0,
    description: "FM-QCF, end frequency ~40\u{2013}42 kHz. Very common and widespread across South Asia. Closely associated with human settlements. Often the first bat to emerge at dusk.",
    echolocates: true,
};

pub const HESPEROPTENUS_BLANFORDI: BatSpecies = BatSpecies {
    id: "hesperoptenus_blanfordi",
    name: "Blanford's Bat",
    scientific_name: "Hesperoptenus blanfordi",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 50_000.0,
    description: "FM-QCF, peak ~32\u{2013}40 kHz. Mainland SE Asia (Myanmar, Thailand, Vietnam). Open forest and over water. Poorly studied acoustically.",
    echolocates: true,
};

pub const IA_IO: BatSpecies = BatSpecies {
    id: "ia_io",
    name: "Great Evening Bat",
    scientific_name: "Ia io",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 11_000.0,
    freq_hi_hz: 32_000.0,
    description: "Very low FM-QCF ~12\u{2013}27 kHz. One of the world\u{2019}s largest vespertilionids (~60 g). Carnivorous aerial hawker preying on large moths and occasionally small birds. South and central China, extending into montane SE Asia.",
    echolocates: true,
};

pub const MYOTIS_MONTIVAGUS: BatSpecies = BatSpecies {
    id: "myotis_montivagus",
    name: "Burmese Whiskered Bat",
    scientific_name: "Myotis montivagus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 100_000.0,
    description: "Broadband FM sweeps, peak ~40\u{2013}55 kHz. Southern Western Ghats and northeastern India. Clutter-adapted gleaning forager of forested hill slopes.",
    echolocates: true,
};

pub const MYOTIS_PILOSUS: BatSpecies = BatSpecies {
    id: "myotis_pilosus",
    name: "Rickett's Big-footed Bat",
    scientific_name: "Myotis pilosus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 27_000.0,
    freq_hi_hz: 75_000.0,
    description: "FM ~71\u{2013}28 kHz, peak ~38\u{2013}41 kHz. East Asia\u{2019}s only confirmed piscivorous bat; trawls rivers using large feet. Endemic to China, also recorded in Korea. Cave roosts near rivers.",
    echolocates: true,
};

pub const MYOTIS_CHINENSIS: BatSpecies = BatSpecies {
    id: "myotis_chinensis",
    name: "Large Myotis",
    scientific_name: "Myotis chinensis",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 100_000.0,
    description: "Broadband FM. Largest Myotis in China (forearm ~65\u{2013}70 mm). Cave-roosting, large maternity colonies in southern and central China.",
    echolocates: true,
};

pub const MYOTIS_DAVIDII: BatSpecies = BatSpecies {
    id: "myotis_davidii",
    name: "David's Myotis",
    scientific_name: "Myotis davidii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 40_000.0,
    freq_hi_hz: 100_000.0,
    description: "FM sweeps, peak ~45\u{2013}55 kHz with geographic variation. Endemic to China. Forest bat. Well-studied for genome analysis of bat-specific adaptations.",
    echolocates: true,
};

pub const HYPSUGO_ALASCHANICUS: BatSpecies = BatSpecies {
    id: "hypsugo_alaschanicus",
    name: "Alashanian Pipistrelle",
    scientific_name: "Hypsugo alaschanicus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM-QCF, peak ~34\u{2013}37 kHz. Previously a subspecies of Savi\u{2019}s Pipistrelle (H. savii). Northern and central China, Korea, Mongolia, Russia. Crevices and buildings.",
    echolocates: true,
};

pub const NYCTALUS_PLANCYI: BatSpecies = BatSpecies {
    id: "nyctalus_plancyi",
    name: "Chinese Noctule",
    scientific_name: "Nyctalus plancyi",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM-QCF, peak ~25\u{2013}34 kHz. Fast high-altitude aerial forager. Endemic to China and Taiwan. Tree hollows. Closely related to N. aviator and N. noctula.",
    echolocates: true,
};

pub const MURINA_LEUCOGASTER: BatSpecies = BatSpecies {
    id: "murina_leucogaster",
    name: "Greater Tube-nosed Bat",
    scientific_name: "Murina leucogaster",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 45_000.0,
    freq_hi_hz: 110_000.0,
    description: "Broadband FM, peak ~45\u{2013}68 kHz. Forest gleaner. Widespread in northern and central China, Korea, Russia, and Japan. Tree hollows and bark.",
    echolocates: true,
};

pub const SCOTOMANES_ORNATUS: BatSpecies = BatSpecies {
    id: "scotomanes_ornatus",
    name: "Harlequin Bat",
    scientific_name: "Scotomanes ornatus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 62_000.0,
    description: "FM-QCF ~43\u{2013}62 kHz start, ~18\u{2013}23 kHz end, peak ~29\u{2013}35 kHz. Large, strikingly patterned bat (black, white, chestnut). Southern and central China. Forested hillsides; rarely encountered.",
    echolocates: true,
};

pub const OTONYCTERIS_HEMPRICHII: BatSpecies = BatSpecies {
    id: "otonycteris_hemprichii",
    name: "Desert Long-eared Bat",
    scientific_name: "Otonycteris hemprichii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 55_000.0,
    description: "Very low-intensity broadband FM ~18\u{2013}55 kHz, peak ~30 kHz; almost undetectable on bat detectors. Specialist scorpion predator using passive listening. Enormous ears (38\u{2013}45 mm). Sahara through Arabia to Central Asia.",
    echolocates: true,
};

pub const PLECOTUS_CHRISTII: BatSpecies = BatSpecies {
    id: "plecotus_christii",
    name: "Christie's Long-eared Bat",
    scientific_name: "Plecotus christii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 60_000.0,
    description: "Very quiet broadband FM, similar to other Plecotus. Gleaning specialist replacing P. auritus in arid Middle Eastern and North African habitats. Ruins and rock crevices in desert environments.",
    echolocates: true,
};

pub const MYOTIS_BLYTHII: BatSpecies = BatSpecies {
    id: "myotis_blythii",
    name: "Lesser Mouse-eared Bat",
    scientific_name: "Myotis blythii",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 85_000.0,
    description: "FM ~25\u{2013}85 kHz, very similar to M. myotis but smaller with slightly higher peak frequency. Widespread from Europe through Turkey, the Levant, Iran, and Central Asia. Key cave bat of the Middle East.",
    echolocates: true,
};

pub const MYOTIS_EMARGINATUS: BatSpecies = BatSpecies {
    id: "myotis_emarginatus",
    name: "Geoffroy's Bat",
    scientific_name: "Myotis emarginatus",
    family: "Vespertilionidae",
    call_type: "FM",
    freq_lo_hz: 35_000.0,
    freq_hi_hz: 115_000.0,
    description: "Broadband FM sweep >100\u{2013}35 kHz. Distinctive notched ear margin. Gleaner specialist. Europe across Turkey, the Levant, Iran, and into Central Asia. Cave-roosting.",
    echolocates: true,
};

pub const EPTESICUS_BOTTAE: BatSpecies = BatSpecies {
    id: "eptesicus_bottae",
    name: "Botta's Serotine",
    scientific_name: "Eptesicus bottae",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 25_000.0,
    freq_hi_hz: 50_000.0,
    description: "FM-QCF, peak ~30\u{2013}35 kHz. Arid Middle Eastern and Central Asian habitats. Arabian Peninsula through Iran and Afghanistan. Rocky desert; buildings and rock crevices.",
    echolocates: true,
};

pub const EPTESICUS_ISABELLINUS: BatSpecies = BatSpecies {
    id: "eptesicus_isabellinus",
    name: "Isabelline Serotine",
    scientific_name: "Eptesicus isabellinus",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 22_000.0,
    freq_hi_hz: 45_000.0,
    description: "FM-QCF, peak ~28\u{2013}32 kHz. Pale sandy serotine of arid North Africa and the Middle East; recently separated from E. serotinus by molecular work. Open landscapes and towns.",
    echolocates: true,
};

pub const PIPISTRELLUS_DESERTI: BatSpecies = BatSpecies {
    id: "pipistrellus_deserti",
    name: "Desert Pipistrelle",
    scientific_name: "Pipistrellus deserti",
    family: "Vespertilionidae",
    call_type: "FM-QCF",
    freq_lo_hz: 38_000.0,
    freq_hi_hz: 55_000.0,
    description: "FM-QCF, peak ~44\u{2013}48 kHz. Small pale pipistrelle of Saharan and Arabian desert margins. Separated from P. kuhlii by slightly higher frequency. Egypt, Israel, Jordan, Saudi Arabia, Yemen.",
    echolocates: true,
};

// ── Pteropodidae ──

pub const ROUSETTUS_AMPLEXICAUDATUS: BatSpecies = BatSpecies {
    id: "rousettus_amplexicaudatus",
    name: "Geoffroy's Rousette",
    scientific_name: "Rousettus amplexicaudatus",
    family: "Pteropodidae",
    call_type: "clicks",
    freq_lo_hz: 10_000.0,
    freq_hi_hz: 70_000.0,
    description: "Tongue-click echolocation for cave navigation. Enormous cave colonies (millions). Widespread across SE Asia. Important seed disperser.",
    echolocates: true,
};

pub const ROUSETTUS_LESCHENAULTII: BatSpecies = BatSpecies {
    id: "rousettus_leschenaultii",
    name: "Leschenault's Rousette",
    scientific_name: "Rousettus leschenaultii",
    family: "Pteropodidae",
    call_type: "clicks",
    freq_lo_hz: 18_000.0,
    freq_hi_hz: 32_000.0,
    description: "Tongue-click echolocation ~18\u{2013}32 kHz. The only South Asian pteropodid with confirmed echolocation. Cave-roosting across India, Sri Lanka, and southern China. Large colonies.",
    echolocates: true,
};

pub const CYNOPTERUS_SPHINX: BatSpecies = BatSpecies {
    id: "cynopterus_sphinx",
    name: "Greater Short-nosed Fruit Bat",
    scientific_name: "Cynopterus sphinx",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "No echolocation; navigates by vision and smell. Tent-roosting in modified palm and screwpine leaves. Important pollinator and seed disperser across South and SE Asia.",
    echolocates: false,
};

pub const EONYCTERIS_SPELAEA: BatSpecies = BatSpecies {
    id: "eonycteris_spelaea",
    name: "Cave Nectar Bat",
    scientific_name: "Eonycteris spelaea",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "No echolocation. Key pollinator of durian, banana, and petai in SE Asia. Cave-roosting; large colonies. Long tongue for probing flowers. India to the Philippines.",
    echolocates: false,
};

pub const PTEROPUS_GIGANTEUS: BatSpecies = BatSpecies {
    id: "pteropus_giganteus",
    name: "Indian Flying Fox",
    scientific_name: "Pteropus giganteus",
    family: "Pteropodidae",
    call_type: "none",
    freq_lo_hz: 0.0,
    freq_hi_hz: 0.0,
    description: "No echolocation. Largest bat in South Asia (wingspan up to 1.5 m). Large noisy roost camps in tall trees. Pakistan, India, Nepal, Bangladesh, Sri Lanka. Important pollinator and seed disperser.",
    echolocates: false,
};

// ══════════════════════════════════════════════════════════════════════════════
// Antarctica — echolocating marine mammals of the Southern Ocean
// ══════════════════════════════════════════════════════════════════════════════
//
// Toothed whales (Odontoceti) echolocate using biosonar clicks produced in
// nasal air sacs and focused by the melon. Frequencies and mechanisms vary
// widely: sperm whales use powerful low-frequency clicks, dolphins and orcas
// use broadband clicks up to 100+ kHz, and porpoises produce narrow-band
// high-frequency (NBHF) clicks above 100 kHz. Baleen whales and seals do
// not echolocate and are listed as non-echolocating, analogous to fruit bats.
//
// Sources:
// - Au (1993) "The Sonar of Dolphins" — dolphin/porpoise biosonar
// - Møhl et al. (2003) JASA: Sperm whale click source parameters
// - Kyhn et al. (2009) JASA: Spectacled porpoise NBHF clicks
// - Johnson et al. (2004) Proc R Soc B: Beaked whale echolocation
// - Schevill & Watkins (1966): Early cetacean bioacoustics

// ── Delphinidae (oceanic dolphins & orcas) ──────────────────────────────────

pub const ORCINUS_ORCA: BatSpecies = BatSpecies {
    id: "orcinus_orca",
    name: "Orca",
    scientific_name: "Orcinus orca",
    family: "Delphinidae",
    call_type: "clicks",
    freq_lo_hz: 500.0,
    freq_hi_hz: 80_000.0,
    description: "Apex predator of the Southern Ocean. Produces broadband echolocation clicks (peak ~20 kHz, energy to 80+ kHz) plus complex pulsed calls and whistles for communication. Antarctic populations include fish-eating Type C, mammal-hunting Type A, and pack-ice specialist Type B. Highly social; pods use distinct vocal dialects.",
    echolocates: true,
};

pub const LAGENORHYNCHUS_CRUCIGER: BatSpecies = BatSpecies {
    id: "lagenorhynchus_cruciger",
    name: "Hourglass Dolphin",
    scientific_name: "Lagenorhynchus cruciger",
    family: "Delphinidae",
    call_type: "clicks",
    freq_lo_hz: 2_000.0,
    freq_hi_hz: 130_000.0,
    description: "The only small dolphin found exclusively in cold Antarctic and sub-Antarctic waters. Named for its striking black-and-white hourglass flank pattern. Produces broadband echolocation clicks. One of the least-studied cetaceans due to its remote pelagic habitat; rarely approaches vessels.",
    echolocates: true,
};

pub const LISSODELPHIS_PERONII: BatSpecies = BatSpecies {
    id: "lissodelphis_peronii",
    name: "Southern Right Whale Dolphin",
    scientific_name: "Lissodelphis peronii",
    family: "Delphinidae",
    call_type: "clicks",
    freq_lo_hz: 1_000.0,
    freq_hi_hz: 100_000.0,
    description: "Slender, finless dolphin of the Southern Ocean — the only dolphin in the Southern Hemisphere lacking a dorsal fin. Jet black above, bright white below. Fast, graceful swimmer found in large schools. Produces broadband clicks for echolocation. Circumpolar in cool temperate to sub-Antarctic waters.",
    echolocates: true,
};

pub const GLOBICEPHALA_MELAS: BatSpecies = BatSpecies {
    id: "globicephala_melas",
    name: "Long-finned Pilot Whale",
    scientific_name: "Globicephala melas",
    family: "Delphinidae",
    call_type: "clicks",
    freq_lo_hz: 1_000.0,
    freq_hi_hz: 80_000.0,
    description: "Large, gregarious dolphin with a bulbous melon and sickle-shaped flippers. Echolocates with broadband clicks; also produces complex social calls and buzzes during deep squid-hunting dives. Found throughout sub-Antarctic waters in large, tightly bonded pods. Prone to mass strandings.",
    echolocates: true,
};

// ── Phocoenidae (porpoises) ─────────────────────────────────────────────────

pub const PHOCOENA_DIOPTRICA: BatSpecies = BatSpecies {
    id: "phocoena_dioptrica",
    name: "Spectacled Porpoise",
    scientific_name: "Phocoena dioptrica",
    family: "Phocoenidae",
    call_type: "NBHF",
    freq_lo_hz: 110_000.0,
    freq_hi_hz: 140_000.0,
    description: "Elusive sub-Antarctic porpoise with distinctive dark eye patches. Uses narrow-band high-frequency (NBHF) clicks around 128–130 kHz — a stealth sonar strategy thought to evade orca predation, as orcas hear poorly above ~100 kHz. Rarely seen alive; mostly known from strandings on Tierra del Fuego and the Falklands.",
    echolocates: true,
};

// ── Physeteridae (sperm whales) ─────────────────────────────────────────────

pub const PHYSETER_MACROCEPHALUS: BatSpecies = BatSpecies {
    id: "physeter_macrocephalus",
    name: "Sperm Whale",
    scientific_name: "Physeter macrocephalus",
    family: "Physeteridae",
    call_type: "clicks",
    freq_lo_hz: 200.0,
    freq_hi_hz: 30_000.0,
    description: "The loudest animal on Earth — produces echolocation clicks exceeding 230 dB re 1µPa. The enormous spermaceti organ acts as an acoustic lens, focusing powerful, directional clicks for hunting giant squid at depths beyond 1,000 m. Also produces codas: rhythmic click patterns used for social communication between matrilineal clans. Males range into Antarctic waters seasonally.",
    echolocates: true,
};

// ── Ziphiidae (beaked whales) ───────────────────────────────────────────────

pub const HYPEROODON_PLANIFRONS: BatSpecies = BatSpecies {
    id: "hyperoodon_planifrons",
    name: "Southern Bottlenose Whale",
    scientific_name: "Hyperoodon planifrons",
    family: "Ziphiidae",
    call_type: "FM clicks",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 70_000.0,
    description: "Deep-diving beaked whale of the circumpolar Southern Ocean. Produces frequency-modulated echolocation clicks during foraging dives exceeding 1,000 m. One of the most commonly sighted beaked whales in Antarctic waters. Bulbous forehead; males develop a pronounced melon with age.",
    echolocates: true,
};

pub const BERARDIUS_ARNUXII: BatSpecies = BatSpecies {
    id: "berardius_arnuxii",
    name: "Arnoux's Beaked Whale",
    scientific_name: "Berardius arnuxii",
    family: "Ziphiidae",
    call_type: "FM clicks",
    freq_lo_hz: 15_000.0,
    freq_hi_hz: 65_000.0,
    description: "Largest beaked whale in the Southern Hemisphere (up to 10 m). Produces FM echolocation clicks during deep foraging dives. Two teeth erupt at the tip of the lower jaw in both sexes. Circumpolar in cold southern waters, occasionally sighted near the ice edge. Closely related to Baird's Beaked Whale of the North Pacific.",
    echolocates: true,
};

pub const MESOPLODON_LAYARDII: BatSpecies = BatSpecies {
    id: "mesoplodon_layardii",
    name: "Strap-toothed Whale",
    scientific_name: "Mesoplodon layardii",
    family: "Ziphiidae",
    call_type: "FM clicks",
    freq_lo_hz: 20_000.0,
    freq_hi_hz: 70_000.0,
    description: "Remarkable beaked whale whose males grow long strap-like teeth that curl over the upper jaw, eventually preventing it from opening fully — yet they still catch squid using suction feeding. Produces FM echolocation clicks for deep-water prey detection. Circumpolar in southern temperate and sub-Antarctic seas.",
    echolocates: true,
};

// ── Balaenopteridae (rorquals) — non-echolocating ──────────────────────────

pub const MEGAPTERA_NOVAEANGLIAE: BatSpecies = BatSpecies {
    id: "megaptera_novaeangliae",
    name: "Humpback Whale",
    scientific_name: "Megaptera novaeangliae",
    family: "Balaenopteridae",
    call_type: "song",
    freq_lo_hz: 80.0,
    freq_hi_hz: 4_000.0,
    description: "No echolocation. Famous for complex, evolving songs lasting up to 30 minutes. Migrates from tropical breeding grounds to Antarctic feeding waters each summer, using bubble-net feeding to corral krill. Songs propagate hundreds of kilometres and shift culturally across populations.",
    echolocates: false,
};

pub const BALAENOPTERA_MUSCULUS: BatSpecies = BatSpecies {
    id: "balaenoptera_musculus",
    name: "Blue Whale",
    scientific_name: "Balaenoptera musculus",
    family: "Balaenopteridae",
    call_type: "infrasonic",
    freq_lo_hz: 10.0,
    freq_hi_hz: 200.0,
    description: "No echolocation. The largest animal ever to have lived. Produces infrasonic calls (10–40 Hz) detectable across ocean basins. Antarctic blue whales (B. m. intermedia) feed on enormous quantities of krill in the Southern Ocean during austral summer. Critically endangered; slowly recovering from whaling.",
    echolocates: false,
};

pub const BALAENOPTERA_BONAERENSIS: BatSpecies = BatSpecies {
    id: "balaenoptera_bonaerensis",
    name: "Antarctic Minke Whale",
    scientific_name: "Balaenoptera bonaerensis",
    family: "Balaenopteridae",
    call_type: "pulse",
    freq_lo_hz: 50.0,
    freq_hi_hz: 9_400.0,
    description: "No echolocation. The most abundant baleen whale in Antarctic waters. Produces a distinctive \"bio-duck\" sound — a repetitive, quacking pulse train — that puzzled researchers for decades before being attributed to this species. Small for a rorqual; frequently seen in pack ice.",
    echolocates: false,
};

pub const BALAENOPTERA_PHYSALUS: BatSpecies = BatSpecies {
    id: "balaenoptera_physalus",
    name: "Fin Whale",
    scientific_name: "Balaenoptera physalus",
    family: "Balaenopteridae",
    call_type: "infrasonic",
    freq_lo_hz: 15.0,
    freq_hi_hz: 850.0,
    description: "No echolocation. Second-largest animal on Earth. Produces powerful 20 Hz pulses that carry across ocean basins — among the loudest sustained biological sounds. Asymmetric colouration: white right lower jaw, dark left. Fast swimmer, nicknamed the 'greyhound of the sea'. Regular summer visitor to Antarctic waters.",
    echolocates: false,
};

// ── Balaenidae (right whales) — non-echolocating ───────────────────────────

pub const EUBALAENA_AUSTRALIS: BatSpecies = BatSpecies {
    id: "eubalaena_australis",
    name: "Southern Right Whale",
    scientific_name: "Eubalaena australis",
    family: "Balaenidae",
    call_type: "moans",
    freq_lo_hz: 50.0,
    freq_hi_hz: 2_500.0,
    description: "No echolocation. Slow, rotund baleen whale with distinctive callosities (rough skin patches colonised by cyamid whale lice). Produces low-frequency moans and up-calls for contact. Once hunted to near-extinction — the 'right' whale to hunt because it floated when dead. Sub-Antarctic; calves in sheltered bays.",
    echolocates: false,
};

// ── Phocidae (true seals) — non-echolocating ───────────────────────────────

pub const LEPTONYCHOTES_WEDDELLII: BatSpecies = BatSpecies {
    id: "leptonychotes_weddellii",
    name: "Weddell Seal",
    scientific_name: "Leptonychotes weddellii",
    family: "Phocidae",
    call_type: "trills",
    freq_lo_hz: 200.0,
    freq_hi_hz: 13_000.0,
    description: "Not echolocation. Southernmost breeding mammal in the world. Produces an extraordinary repertoire of underwater sounds: eerie descending trills, chirps, and otherworldly whistles audible through the ice. Dives to 600+ m and maintains breathing holes by grinding ice with its teeth. Lives on fast ice year-round.",
    echolocates: false,
};

pub const HYDRURGA_LEPTONYX: BatSpecies = BatSpecies {
    id: "hydrurga_leptonyx",
    name: "Leopard Seal",
    scientific_name: "Hydrurga leptonyx",
    family: "Phocidae",
    call_type: "trills",
    freq_lo_hz: 200.0,
    freq_hi_hz: 8_000.0,
    description: "Not echolocation. Apex predator of the Antarctic pack ice. Males produce haunting, pulsing underwater trills and low-frequency broadcast calls during the breeding season — audible over great distances beneath the ice. Feeds on penguins, seals, krill, and fish. Solitary; sinuous and powerful.",
    echolocates: false,
};
