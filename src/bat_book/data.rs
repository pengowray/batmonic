use super::types::{BookEntryDef, BatBookManifest, BatBookRegion, Commonness};
use super::species;

// ══════════════════════════════════════════════════════════════════════════════
// Global book — family-level entries
// ══════════════════════════════════════════════════════════════════════════════

const GLOBAL_BOOK: &[BookEntryDef] = &[
    BookEntryDef { species: &species::RHINOLOPHIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDERIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::VESPERTILIONIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MOLOSSIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::EMBALLONURIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PHYLLOSTOMIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MORMOOPIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MINIOPTERIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::NYCTERIDAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MEGADERMATIDAE, commonness: None, description: None, name: None },
    // Non-echolocating (will be sorted to end by get_manifest)
    BookEntryDef { species: &species::PTEROPODIDAE, commonness: None, description: None, name: None },
];

// ══════════════════════════════════════════════════════════════════════════════
// VIC, Australia — species-level entries sorted by commonness
// ══════════════════════════════════════════════════════════════════════════════

const VIC_AUSTRALIA_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::CHALINOLOBUS_GOULDII,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Widespread and abundant across Victoria. Roosts in tree hollows, buildings, and bat boxes. Alternating call frequencies are distinctive."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHALINOLOBUS_MORIO,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Common across southern Australia. Small, dark bat roosting in tree hollows and buildings. Higher frequency calls than Gould's Wattled Bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_GEOFFROYI,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Australia's most widespread bat. Very quiet, broadband FM calls; often difficult to detect acoustically. Gleaning insectivore with large ears."),
        name: None,
    },
    BookEntryDef {
        species: &species::AUSTRONOMUS_AUSTRALIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Australia's largest insectivorous bat. Loud, low-frequency calls audible to some humans. Fast, high-flying open-air forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_VULTURNUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("One of Australia's smallest bats (~4 g). Common in forests and urban areas throughout Victoria. High-frequency calls."),
        name: None,
    },
    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::VESPADELUS_REGULUS,
        commonness: Some(Commonness::Common),
        description: Some("Small forest bat found across southern Australia. Roosts in tree hollows. Call frequency overlaps with Little Forest Bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_GOULDI,
        commonness: Some(Commonness::Common),
        description: Some("Common in forests of eastern Australia. Very quiet calls, similar to Lesser Long-eared Bat. Distinguished by larger size and habitat preference."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_DARLINGTONI,
        commonness: Some(Commonness::Common),
        description: Some("Largest Vespadelus species. Found in wet and dry forests of south-eastern Australia including Tasmania."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_ORIANAE_OCEANENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Cave-roosting bat found along eastern Australia. Fast, agile flier. Maternity cave near Bairnsdale. Vulnerable in Victoria."),
        name: None,
    },
    BookEntryDef {
        species: &species::OZIMOPS_PLANICEPS,
        commonness: Some(Commonness::Common),
        description: Some("Small free-tailed bat of south-eastern Australia. Roosts in tree hollows and buildings. Rapid, direct flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::OZIMOPS_RIDEI,
        commonness: Some(Commonness::Common),
        description: Some("Widespread across eastern Australian coasts. Similar to Southern Free-tailed Bat but slightly higher frequency calls."),
        name: None,
    },
    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::FALSISTRELLUS_TASMANIENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large vesper bat of south-eastern forests. Roosts in tree hollows. Vulnerable (IUCN). Distinctive mid-range frequency calls."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTOREPENS_ORION,
        commonness: Some(Commonness::Uncommon),
        description: Some("Robust bat of south-eastern coastal forests. Narrow frequency range distinctive. Roosts in tree hollows."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTOREPENS_BALSTONI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Widespread across inland Australia. Found in drier regions of northern and western Victoria. Similar frequency to Gould's Wattled Bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_MACROPUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Australia's only fishing bat. Trawls water surfaces with large feet. Found near rivers, lakes, and dams. Very quiet calls."),
        name: None,
    },
    BookEntryDef {
        species: &species::SACCOLAIMUS_FLAVIVENTRIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large, fast-flying bat with glossy black fur and yellow belly. Migratory; visits Victoria seasonally. High-altitude forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_MEGAPHYLLUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Constant-frequency echolocation using distinctive horseshoe-shaped noseleaf. Cave-roosting. Found in forests of eastern and southern Victoria."),
        name: None,
    },
    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::NYCTOPHILUS_MAJOR,
        commonness: Some(Commonness::Rare),
        description: Some("Formerly N. timoriensis. Rare in Victoria, restricted to drier woodlands. Very quiet gleaning calls. Vulnerable."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_BAVERSTOCKI,
        commonness: Some(Commonness::Rare),
        description: Some("Small bat of inland Australia. In Victoria, restricted to the semi-arid northwest (Mallee region)."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTEANAX_RUEPPELLII,
        commonness: Some(Commonness::Rare),
        description: Some("Large, robust bat of eastern coastal forests. Rare in Victoria, mainly in far east Gippsland. Aggressive predator of large insects and small vertebrates."),
        name: None,
    },
    // ── Endangered ───────────────────────────────────────────────
    BookEntryDef {
        species: &species::MINIOPTERUS_ORIANAE_BASSANII,
        commonness: Some(Commonness::Endangered),
        description: Some("Critically Endangered (EPBC Act). Dependent on a single maternity cave near Warrnambool. Population <50 individuals. Southwest Victoria only."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_CORBENI,
        commonness: Some(Commonness::Endangered),
        description: Some("Vulnerable (EPBC Act). Extremely rare in Victoria; restricted to northwest Mallee (Hattah-Kulkyne, Gunbower). Possibly <50 individuals in VIC."),
        name: None,
    },
    // ── Non-echolocating (sorted to end by get_manifest) ────────
    BookEntryDef {
        species: &species::PTEROPUS_POLIOCEPHALUS,
        commonness: Some(Commonness::Rare),
        description: Some("Australia's largest bat (wingspan ~1 m). Does not echolocate. Camps in colonies along waterways. Vulnerable (EPBC Act). Pollinator and seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTEROPUS_SCAPULATUS,
        commonness: Some(Commonness::Vagrant),
        description: Some("Seasonal visitor to northern Victoria. Does not echolocate. Nomadic, following eucalypt flowering. Occasionally camps at Swan Hill and Numurkah."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// Europe — species-level entries sorted by commonness
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Dietz, Helversen & Nill (2009): Bats of Britain, Europe and Northwest Africa
// - Russ (2012): British Bat Calls: A Guide to Species Identification
// - Barataud (2015): Acoustic Ecology of European Bats

const EUROPE_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::PIPISTRELLUS_PIPISTRELLUS,
        commonness: Some(Commonness::VeryCommon),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_PYGMAEUS,
        commonness: Some(Commonness::VeryCommon),
        description: None,
        name: None,
    },
    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::PIPISTRELLUS_NATHUSII,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_KUHLII,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_DAUBENTONII,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_NATTERERI,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_MYSTACINUS,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_BRANDTII,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_MYOTIS,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTALUS_NOCTULA,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTALUS_LEISLERI,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_SEROTINUS,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_NILSSONII,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::PLECOTUS_AURITUS,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_FERRUMEQUINUM,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_HIPPOSIDEROS,
        commonness: Some(Commonness::Common),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_SCHREIBERSII,
        commonness: Some(Commonness::Common),
        description: Some("Fast, agile cave-dweller found across southern Europe. Long, narrow wings. Formerly one species; now split into several. Sensitive to cave disturbance."),
        name: None,
    },
    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::BARBASTELLA_BARBASTELLUS,
        commonness: Some(Commonness::Uncommon),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::PLECOTUS_AUSTRIACUS,
        commonness: Some(Commonness::Uncommon),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::VESPERTILIO_MURINUS,
        commonness: Some(Commonness::Uncommon),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_BECHSTEINII,
        commonness: Some(Commonness::Uncommon),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_DASYCNEME,
        commonness: Some(Commonness::Uncommon),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_TENIOTIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Europe's only free-tailed bat. Loud, low-frequency calls audible to humans. Fast, high-altitude forager. Restricted to the Mediterranean; roosts in cliff crevices and tall buildings."),
        name: None,
    },
    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_EURYALE,
        commonness: Some(Commonness::Rare),
        description: None,
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTALUS_LASIOPTERUS,
        commonness: Some(Commonness::Rare),
        description: None,
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// Costa Rica — species-level entries, tiered by commonness
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Jung et al. (2007): Echolocation calls in Central American emballonurids
// - Leiser-Miller & Santana (2021): Phyllostomid echolocation (Costa Rica data)
// - Gessinger et al. (2019): CF-FM echolocation of Lonchorhina aurita
// - Zamora-Gutierrez et al. (2016): Acoustic identification of Mexican bats
// - Rydell et al. (2002): Acoustic identification of Yucatan bats
//
// Phyllostomidae are low-intensity "whispering" echolocators, typically
// detectable only within a few meters. Descriptions note this limitation.

const COSTA_RICA_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    // Easily detected species with loud calls

    BookEntryDef {
        species: &species::SACCOPTERYX_BILINEATA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant in lowland forests. Roosts on tree trunks and building walls. Alternates ~45/48 kHz. Vocal learner with complex song repertoire."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_MOLOSSUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant in buildings and urban areas. Alternating QCF at ~34.5/39.6 kHz. One of the first species heard at dusk. Open-space aerial hawker."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_MESOAMERICANUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Long CF at ~61 kHz with Doppler compensation\u{2014}the ONLY high-duty-cycle echolocator in the New World. Unmistakable call. Huge cave colonies."),
        name: None,
    },
    BookEntryDef {
        species: &species::CAROLLIA_PERSPICILLATA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("One of the most abundant Neotropical bats. Peak ~71 kHz. Low-intensity whispering calls\u{2014}detectable only within a few meters. Key seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::ARTIBEUS_JAMAICENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Very common frugivore. Peak ~56 kHz. Variable intensity; not always a quiet whisperer. Important fig seed disperser throughout lowland forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_BRASILIENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Extremely flexible acoustics: QCF 49\u{2013}70 kHz in open space, drops to 25\u{2013}40 kHz near objects. Forms massive colonies. Fast, high-altitude forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLOSSOPHAGA_SORICINA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant nectarivore. Peak ~80 kHz. Low-intensity calls\u{2014}hard to detect beyond a few meters. Uses echolocation to find flowers with acoustic reflectors."),
        name: None,
    },
    BookEntryDef {
        species: &species::DESMODUS_ROTUNDUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Common near livestock. Peak ~55 kHz. Relatively long calls for a phyllostomid (~5.5 ms). Low-intensity. Obligate blood-feeder with infrared-sensing nose pits."),
        name: None,
    },

    // ── Common ───────────────────────────────────────────────────

    BookEntryDef {
        species: &species::RHYNCHONYCTERIS_NASO,
        commonness: Some(Commonness::Common),
        description: Some("Tiny bat roosting in lines along riverbanks. CF-FM with peak at ~47 kHz. Drops from ~100 to ~67 kHz during prey pursuit. Cryptic bark-like camouflage."),
        name: None,
    },
    BookEntryDef {
        species: &species::BALANTIOPTERYX_PLICATA,
        commonness: Some(Commonness::Common),
        description: Some("Open-area forager near caves and buildings. Long QCF (14\u{2013}20 ms) at ~43 kHz. Displays jamming avoidance in groups by shifting peak frequency."),
        name: None,
    },
    BookEntryDef {
        species: &species::PEROPTERYX_MACROTIS,
        commonness: Some(Commonness::Common),
        description: Some("Multiharmonic QCF at ~40 kHz (2nd harmonic). Found near caves and rock shelters. Distinctive musky odor."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_DAVYI,
        commonness: Some(Commonness::Common),
        description: Some("CF-FM at ~67 kHz with sweep to ~51 kHz. Wing membranes fused across back (naked-backed appearance). Cave-roosting; often with P. mesoamericanus."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_GYMNONOTUS,
        commonness: Some(Commonness::Common),
        description: Some("CF at ~54\u{2013}57 kHz. Largest Pteronotus. Similar to P. davyi but lower frequency. Cave-dwelling."),
        name: None,
    },
    BookEntryDef {
        species: &species::MORMOOPS_MEGALOPHYLLA,
        commonness: Some(Commonness::Common),
        description: Some("Bizarre leaf-chin face. Fundamental suppressed; 2nd harmonic at ~67 kHz dominates recordings. Large cave colonies. Ghost-like appearance in flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::NOCTILIO_LEPORINUS,
        commonness: Some(Commonness::Common),
        description: Some("Large fishing bat. Long CF at 53\u{2013}56 kHz + FM sweep. Rakes water with large clawed feet to catch fish. Found along rivers, lakes, and coasts."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_SINALOAE,
        commonness: Some(Commonness::Common),
        description: Some("QCF at ~34 kHz. Shifts frequency up ~6 kHz in urban noise (Lombard effect). Larger than M. molossus. Open-space forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_RUFUS,
        commonness: Some(Commonness::Common),
        description: Some("Large molossid with low-frequency QCF at ~25\u{2013}26 kHz. Roosts in buildings and hollow trees. Fast, direct flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_NIGRICANS,
        commonness: Some(Commonness::Common),
        description: Some("Highly plastic calls: narrowband ~7 ms in open space; broadband FM in clutter. Peak ~50 kHz. Common in forests and urban edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::ARTIBEUS_LITURATUS,
        commonness: Some(Commonness::Common),
        description: Some("Large frugivore. Lower peak (~52 kHz) than A. jamaicensis. Low-intensity. Prominent facial stripes. Important pollinator and seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::STURNIRA_LILIUM,
        commonness: Some(Commonness::Common),
        description: Some("Frugivore with well-documented peak at ~66.5 kHz. Low-intensity FM. Yellow shoulder epaulettes in males. Common in forest and edge habitats."),
        name: None,
    },
    BookEntryDef {
        species: &species::URODERMA_BILOBATUM,
        commonness: Some(Commonness::Common),
        description: Some("Tent-roosting frugivore. Bites leaf ribs to create tent roosts. Peak ~70 kHz. Low-intensity nasal FM\u{2014}hard to detect. Lowland forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::CAROLLIA_CASTANEA,
        commonness: Some(Commonness::Common),
        description: Some("Higher peak (~78 kHz) than C. perspicillata. Low-intensity FM. Frugivore preferring understory fruits. Common in wet lowland forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::CAROLLIA_BREVICAUDA,
        commonness: Some(Commonness::Common),
        description: Some("Intermediate peak (~73 kHz) between C. perspicillata and C. castanea. Low-intensity FM. Frugivore. Premontane and montane forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLOSSOPHAGA_COMMISSARISI,
        commonness: Some(Commonness::Common),
        description: Some("Nectarivore. Slightly lower peak (~75 kHz) than G. soricina. Low-intensity FM. Important pollinator of many tropical plants."),
        name: None,
    },
    BookEntryDef {
        species: &species::TRACHOPS_CIRRHOSUS,
        commonness: Some(Commonness::Common),
        description: Some("Famous frog-eating bat. Locates prey by listening to mating calls. Peak ~70 kHz. Low-intensity FM\u{2014}hard to detect. Warty lips for gripping frogs."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHYLLOSTOMUS_HASTATUS,
        commonness: Some(Commonness::Common),
        description: Some("Large omnivore. One of the lowest-frequency phyllostomids (~47 kHz peak). Low-intensity FM. Harem groups in caves and hollow trees."),
        name: None,
    },
    BookEntryDef {
        species: &species::DERMANURA_PHAEOTIS,
        commonness: Some(Commonness::Common),
        description: Some("Small frugivore. Peak ~75 kHz. Low-intensity FM\u{2014}detectable only within a few meters. Common in lowland and premontane forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::MICRONYCTERIS_MICROTIS,
        commonness: Some(Commonness::Common),
        description: Some("Gleaning insectivore. Very short broadband FM (0.3\u{2013}1 ms) at ~90\u{2013}100 kHz. Ultra-low intensity\u{2014}barely detectable beyond 2\u{2013}3 m. Can find motionless prey."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_BRASILIENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Peak ~54\u{2013}60 kHz. Source level ~101\u{2013}106 dB SPL. FM-QCF. Frequency varies with temperature. Forest edges and open areas."),
        name: None,
    },

    // ── Uncommon ─────────────────────────────────────────────────

    BookEntryDef {
        species: &species::SACCOPTERYX_LEPTURA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Higher frequency (~50 kHz) than S. bilineata. Similar QCF structure. Thinner dorsal stripes. Less common; found in lowland forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::CORMURA_BREVIROSTRIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Unusual: most energy in 5th harmonic at ~68 kHz. Forest-interior forager. Multiharmonic calls."),
        name: None,
    },
    BookEntryDef {
        species: &species::PEROPTERYX_KAPPLERI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Lower frequency (~32 kHz) than P. macrotis. 2nd harmonic dominant. Near caves and rocky outcrops in forested areas."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_PERSONATUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Highest frequency Pteronotus: initial CF ~83 kHz, terminal ~68 kHz. Doppler-shift compensation. Cave-roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::NOCTILIO_ALBIVENTRIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Higher CF (~75 kHz) than N. leporinus. Trawls insects and small fish from water. Less common than greater bulldog bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_BONDAE,
        commonness: Some(Commonness::Uncommon),
        description: Some("QCF at ~33 kHz. Open-space forager. Roosts in buildings. Slightly lower frequency than M. molossus."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_COIBENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("QCF at ~35 kHz. Originally described from Coiba Island, Panama. Open-space forager. Smaller than other Molossus species."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_AURIPENDULUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large molossid. Alternating QCF at ~23\u{2013}26 kHz. High, fast flight above canopy. Long-duration narrowband calls."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_GLAUCINUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Very low frequency (~22\u{2013}25 kHz) QCF. Large bat with long narrow wings. High-altitude forager above canopy."),
        name: None,
    },
    BookEntryDef {
        species: &species::CYNOMOPS_GREENHALLI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Low frequency (~22 kHz) open-space forager. Flat face with forward-pointing nostrils. Roosts in buildings and hollow trees."),
        name: None,
    },
    BookEntryDef {
        species: &species::PROMOPS_CENTRALIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Distinctive upward-modulated QCF (unusual for molossids). Alternating pairs at ~30/35 kHz. Easily recognized on bat detector."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTINOMOPS_LATICAUDATUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Three-frequency alternation (~26.7, 28.7, 32.4 kHz). Open-space forager. Roosts in rock crevices and buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_KEAYSI,
        commonness: Some(Commonness::Uncommon),
        description: Some("High repetition rates (15\u{2013}20/s). Short FM calls (~2.5 ms). Peak ~55 kHz. Found in highlands and cloud forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_RIPARIUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Steep broadband FM sweep from ~120 to ~50 kHz. Short calls (~2 ms). Forages near streams and forest edges. Recorded in Costa Rica."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_ELEGANS,
        commonness: Some(Commonness::Uncommon),
        description: Some("High-frequency FM (~55 kHz peak). Difficult to distinguish from M. nigricans acoustically. Small Myotis of lowland forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_FURINALIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Lower frequency (~43 kHz) than E. brasiliensis. FM-QCF. More FM in cluttered habitats. Forest edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_BLOSSEVILLII,
        commonness: Some(Commonness::Uncommon),
        description: Some("Open-air forager. Peak ~42 kHz. FM-QCF. Migratory. Roosts solitarily in foliage. Distinctive reddish fur."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_EGA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Lower peak (~35 kHz) than L. blossevillii. FM-QCF. Roosts in palm fronds. Open-air forager around street lights."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHOGEESSA_TUMIDA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small vespertilionid. Broadband FM + QCF termination at ~48 kHz. Forages low in forest gaps and edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::LONCHORHINA_AURITA,
        commonness: Some(Commonness::Uncommon),
        description: Some("UNIQUE phyllostomid with CF-FM calls. Long CF at ~45 kHz (3rd harmonic). Longest phyllostomid calls (up to 8.7 ms). Extremely long nose-leaf. Cave-roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHYLLOSTOMUS_DISCOLOR,
        commonness: Some(Commonness::Uncommon),
        description: Some("Omnivore. Peak ~55 kHz. Low-intensity FM. Large colonies in hollow trees. Best hearing at 20 kHz. Low-frequency for a phyllostomid."),
        name: None,
    },
    BookEntryDef {
        species: &species::LOPHOSTOMA_SILVICOLUM,
        commonness: Some(Commonness::Uncommon),
        description: Some("Gleaning insectivore that modifies termite nests into roosts. Peak ~70 kHz. Low-intensity FM\u{2014}detectable only within a few meters."),
        name: None,
    },
    BookEntryDef {
        species: &species::ANOURA_GEOFFROYI,
        commonness: Some(Commonness::Uncommon),
        description: Some("High-altitude nectarivore. Peak ~70 kHz. Low-intensity FM. Cloud forests and highlands. Important pollinator."),
        name: None,
    },
    BookEntryDef {
        species: &species::CENTURIO_SENEX,
        commonness: Some(Commonness::Uncommon),
        description: Some("Bizarre wrinkled face with retractable skin mask. Peak ~65 kHz. Relatively long calls for a stenodermatine (1\u{2013}3 ms). Frugivore. Low-intensity."),
        name: None,
    },

    // ── Rare ─────────────────────────────────────────────────────

    BookEntryDef {
        species: &species::DICLIDURUS_ALBUS,
        commonness: Some(Commonness::Rare),
        description: Some("Distinctive white fur. Narrowband QCF at ~24 kHz. Rarely encountered. High-altitude open-space forager. One of the most striking-looking bats."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_FUSCUS,
        commonness: Some(Commonness::Rare),
        description: Some("Large vespertilionid. Peak ~30 kHz. FM-QCF. At southern edge of range in Costa Rica. Uncommon in highlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::VAMPYRUM_SPECTRUM,
        commonness: Some(Commonness::Rare),
        description: Some("Largest bat in the Americas (wingspan ~1 m). Peak ~70 kHz. Low-intensity FM\u{2014}very difficult to detect acoustically. Carnivorous: preys on birds and other bats."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHROTOPTERUS_AURITUS,
        commonness: Some(Commonness::Rare),
        description: Some("Carnivorous gleaner. Peak ~77 kHz. Short FM (0.8\u{2013}1.4 ms). Low-intensity\u{2014}hard to detect beyond a few meters. Large ears; hunts other bats and rodents."),
        name: None,
    },
    BookEntryDef {
        species: &species::MACROPHYLLUM_MACROPHYLLUM,
        commonness: Some(Commonness::Rare),
        description: Some("Unusual trawling phyllostomid. Louder than most relatives (~101 dB SPL). Peak ~85 kHz. Large feet for grabbing insects from water surfaces."),
        name: None,
    },
    BookEntryDef {
        species: &species::MICRONYCTERIS_HIRSUTA,
        commonness: Some(Commonness::Rare),
        description: Some("Gleaning insectivore. Lower peak (~52 kHz) than M. microtis. Low-intensity FM. Documented from Costa Rica. Forest interior."),
        name: None,
    },
    BookEntryDef {
        species: &species::MIMON_CRENULATUM,
        commonness: Some(Commonness::Rare),
        description: Some("Gleaning insectivore. Peak ~75 kHz. Low-intensity FM. Now Gardnerycteris crenulatum. Forest understory."),
        name: None,
    },
    BookEntryDef {
        species: &species::TONATIA_SAUROPHILA,
        commonness: Some(Commonness::Rare),
        description: Some("Gleaning insectivore/carnivore. Peak ~65 kHz. Low-intensity FM. Forest interior specialist. Roosts in hollow trees."),
        name: None,
    },
    BookEntryDef {
        species: &species::LAMPRONYCTERIS_BRACHYOTIS,
        commonness: Some(Commonness::Rare),
        description: Some("Rare gleaning insectivore. Peak ~75 kHz. Low-intensity FM. Poorly documented acoustically. Yellow throat patches."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLYPHONYCTERIS_SYLVESTRIS,
        commonness: Some(Commonness::Rare),
        description: Some("Rare gleaner. Peak ~85 kHz. Ultra-short broadband FM (0.3\u{2013}1 ms). Very low intensity. Forest interior."),
        name: None,
    },
    BookEntryDef {
        species: &species::TRINYCTERIS_NICEFORI,
        commonness: Some(Commonness::Rare),
        description: Some("Low-intensity gleaner. Peak ~80 kHz. Multiharmonic FM. Forest understory specialist. Rarely captured or detected."),
        name: None,
    },
    BookEntryDef {
        species: &species::HYLONYCTERIS_UNDERWOODI,
        commonness: Some(Commonness::Rare),
        description: Some("Very small nectarivore. High frequency peak ~90 kHz. Low-intensity FM. Montane cloud forests. Poorly known acoustically."),
        name: None,
    },
    BookEntryDef {
        species: &species::MESOPHYLLA_MACCONNELLI,
        commonness: Some(Commonness::Rare),
        description: Some("Tiny (5\u{2013}7 g) with the highest peak frequency of any phyllostomid (~100\u{2013}118 kHz). Ultra-low intensity. Tent-roosting frugivore."),
        name: None,
    },
    BookEntryDef {
        species: &species::ECTOPHYLLA_ALBA,
        commonness: Some(Commonness::Rare),
        description: Some("Iconic tiny white bat. Peak ~75 kHz. Low-intensity FM. Roosts in Heliconia leaf tents. Endemic to Central America. Specializes on one fig species."),
        name: None,
    },
    BookEntryDef {
        species: &species::THYROPTERA_TRICOLOR,
        commonness: Some(Commonness::Rare),
        description: Some("Suction-cup disks for roosting in rolled Heliconia leaves. Extremely low intensity\u{2014}barely detectable at <1 m. Distinctive social calls for roost-finding."),
        name: None,
    },
    BookEntryDef {
        species: &species::NATALUS_MEXICANUS,
        commonness: Some(Commonness::Rare),
        description: Some("Among the highest frequency bats: peak ~100\u{2013}130 kHz. Very low intensity\u{2014}barely detectable beyond 50 cm. Delicate, cave-roosting. Formerly N. stramineus."),
        name: None,
    },
    BookEntryDef {
        species: &species::BAUERUS_DUBIAQUERCUS,
        commonness: Some(Commonness::Rare),
        description: Some("Very quiet calls (~35 kHz peak). Plecotus-like gleaning insectivore. Rare and poorly known. Montane forests."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// Japan — species-level entries sorted by commonness
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Fukui et al. (2004) Zool Sci: Acoustic ID of eight Hokkaido species
// - Funakoshi & Uchida (1978): R. ferrumequinum nippon CF frequency
// - Hiryu et al. (2006): R. pumilus geographic variation on Okinawa
// - Fujioka et al. (2014): CNN bat species ID system for 30 Japanese species
// - IUCN Red List; Ministry of the Environment (Japan) Red Data Book

const JAPAN_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::PIPISTRELLUS_ABRAMUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Japan's most common urban bat. FM sweeps ~80\u{2013}95 kHz to terminal ~40 kHz, peak ~52 kHz. Roosts in buildings, forages around streetlights. Found throughout the archipelago."),
        name: None,
    },

    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_FERRUMEQUINUM_NIPPON,
        commonness: Some(Commonness::Common),
        description: Some("CF-FM calls with diagnostic CF2 at ~65\u{2013}69 kHz. FM/CF/FM structure. Widespread from Hokkaido to Kyushu. Cave, mine, and tunnel roosts. Key species for Doppler-shift research."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_CORNUTUS,
        commonness: Some(Commonness::Common),
        description: Some("CF-FM calls with peak CF ~103\u{2013}111 kHz, increasing from north to south (103\u{2013}104 kHz in Aomori, 108\u{2013}111 kHz on Amami-Oshima). Endemic to Japan. Caves and buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_FULIGINOSUS,
        commonness: Some(Commonness::Common),
        description: Some("FM downward sweeps, terminal frequency ~44\u{2013}50 kHz. Shows geographic variation in peak frequency among colonies. Fast agile flier. Large cave maternity colonies. Honshu to Ryukyus."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_MACRODACTYLUS,
        commonness: Some(Commonness::Common),
        description: Some("Steep FM sweeps ~90\u{2013}40 kHz, peak ~50 kHz. Trawling insectivore using large feet to gaff prey from water surfaces. Rivers and streams throughout Japan."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPERTILIO_SINENSIS,
        commonness: Some(Commonness::Common),
        description: Some("FM-QCF calls, peak ~24 kHz, max ~46 kHz. Steep FM into shallow QCF tail. Migratory species with seasonal movements through the archipelago. Hokkaido to Kyushu."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTALUS_AVIATOR,
        commonness: Some(Commonness::Common),
        description: Some("FM-QCF calls, peak ~21 kHz, duration ~12 ms\u{2014}longest among Hokkaido bats. Japan's largest insectivorous bat. Forages up to 300 m altitude. Occasionally preys on migrating birds."),
        name: None,
    },
    BookEntryDef {
        species: &species::PLECOTUS_SACRIMONTIS,
        commonness: Some(Commonness::Common),
        description: Some("Low-intensity FM calls, start ~46 kHz, peak ~41 kHz, end ~23 kHz. Gleaning forager specialising in fluttering moths via passive listening. Endemic to Japan. Forest-dwelling."),
        name: None,
    },
    BookEntryDef {
        species: &species::MURINA_HILGENDORFI,
        commonness: Some(Commonness::Common),
        description: Some("Ultra-broadband FM sweeps ~145\u{2013}165 kHz down to ~45\u{2013}55 kHz. Forest gleaner. Hokkaido to Kyushu. Roosts in tree hollows and dead curled leaves."),
        name: None,
    },
    BookEntryDef {
        species: &species::MURINA_USSURIENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Ultra-broadband FM sweeps similar to M. hilgendorfi. Tiny bat (4\u{2013}8 g). Remarkably hibernates under snow. Roosts in curled dead leaves. Hokkaido and Honshu."),
        name: None,
    },
    BookEntryDef {
        species: &species::MURINA_SILVATICA,
        commonness: Some(Commonness::Common),
        description: Some("Broadband FM calls typical of Murina. Distribution spans ~2000 km north\u{2013}south across Japan. Sometimes considered conspecific with M. ussuriensis. Curled-leaf roosts."),
        name: None,
    },

    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_PUMILUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("CF-FM calls with CF ~80\u{2013}90 kHz. Shows 5\u{2013}8 kHz dialect difference between northern and southern Okinawa populations, maintained by maternal transmission. Central and southern Ryukyus."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_IMAIZUMII,
        commonness: Some(Commonness::Uncommon),
        description: Some("CF-FM calls at frequency intermediate between R. cornutus and R. pumilus (~94\u{2013}108 kHz). Temperate forests on Honshu and Shikoku. Taxonomic status debated."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_FUSCUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("FM calls slightly higher frequency than M. fuliginosus due to smaller body size. Ryukyu Islands and southern Kyushu. Cave-roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_IKONNIKOVI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Steep FM sweeps, peak ~50.6 kHz, start ~90 kHz, end ~43 kHz, duration ~2 ms. Small forest bat of Hokkaido and northern Honshu."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_BOMBINUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Very broadband FM calls sweeping from ~150 kHz down to ~20 kHz. Gleaning insectivore, hawks near vegetation. Forests of Kyushu and other regions."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_FRATER,
        commonness: Some(Commonness::Uncommon),
        description: Some("Brief FM pulses ~110\u{2013}50 kHz, duration ~3.5 ms. Edge-space forager near cliffs and caves. Honshu and Kyushu."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_NILSSONII_JP,
        commonness: Some(Commonness::Uncommon),
        description: Some("FM-QCF calls, peak ~30.5 kHz, duration ~6 ms, bandwidth ~32 kHz. Hokkaido and northern Honshu. The most northerly-ranging bat in the world."),
        name: None,
    },

    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_PERDITUS,
        commonness: Some(Commonness::Rare),
        description: Some("CF-FM calls, peak CF ~92\u{2013}98 kHz (92\u{2013}93 on Iriomote, 96\u{2013}98 on Ishigaki). Endemic to the Yaeyama Islands. Forest-dwelling, cave-roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::HIPPOSIDEROS_TURPIS,
        commonness: Some(Commonness::Rare),
        description: Some("CF calls typical of hipposiderids, ~65\u{2013}80 kHz. Southern Ryukyu Islands (Ishigaki, Iriomote, Miyako). Limestone cave roosts. Feeds mainly on beetles."),
        name: None,
    },
    BookEntryDef {
        species: &species::TAPHOZOUS_MELANOPOGON,
        commonness: Some(Commonness::Rare),
        description: Some("Low-intensity FM calls, dominant ~29\u{2013}33 kHz with four harmonics. Marginal occurrence in Japan (Ryukyu Islands). Roosts on rock walls and in caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_INSIGNIS,
        commonness: Some(Commonness::Rare),
        description: Some("Low-frequency FM-QCF calls ~18\u{2013}25 kHz. Fast, high-flying open-air forager. Western Japan. Roosts in rock crevices and buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_LATOUCHEI,
        commonness: Some(Commonness::Rare),
        description: Some("Echolocation ~20 kHz. High-altitude flier, very difficult to capture. Known in Japan from a single specimen on Amami-Oshima (1985). IUCN Data Deficient."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_PRUINOSUS,
        commonness: Some(Commonness::Rare),
        description: Some("FM sweeps typical of Myotis. Endemic to Japan (Honshu, Shikoku, Kyushu). Forest-dwelling, roosts in tree hollows and buildings. Named for frosted fur."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_FORMOSUS,
        commonness: Some(Commonness::Rare),
        description: Some("Steep downward FM calls. Distinctive orange-brown coloring. Tsushima Island and western Japan. Forest-dwelling insectivore."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_HOSONOI,
        commonness: Some(Commonness::Rare),
        description: Some("FM calls typical of Myotis. Endemic to Japan (Honshu). Cave-dwelling. Poorly studied species."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_GRACILIS,
        commonness: Some(Commonness::Rare),
        description: Some("FM calls typical of small Myotis. Endemic to Japan (Honshu). Forest-dwelling. Limited published acoustic data."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_OZENSIS,
        commonness: Some(Commonness::Rare),
        description: Some("FM calls typical of Myotis. Endemic to central Honshu mountains. Cave-dwelling. Very limited distribution."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_ENDOI,
        commonness: Some(Commonness::Rare),
        description: Some("FM-QCF calls similar to P. abramus but slightly lower frequency. Endemic to Japan (Honshu). Temperate forests at 100\u{2013}1500 m. IUCN Near Threatened."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_JAPONENSIS,
        commonness: Some(Commonness::Rare),
        description: Some("FM-QCF calls similar to E. nilssonii. Endemic to Japan (Honshu, Shikoku, Kyushu). Forest-dwelling, roosts in tree hollows and buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::BARBASTELLA_LEUCOMELAS,
        commonness: Some(Commonness::Rare),
        description: Some("FM calls in two alternating types: type A at 32\u{2013}37 kHz, type B at 38\u{2013}45 kHz. Very elusive. Rare in Japan (Honshu). Forest-dwelling."),
        name: None,
    },
    BookEntryDef {
        species: &species::MURINA_RYUKYUANA,
        commonness: Some(Commonness::Rare),
        description: Some("Broadband FM calls typical of Murina. Endemic to the Ryukyu Islands. Recently described species from Okinawa. Forest-dwelling."),
        name: None,
    },

    // ── Endangered ───────────────────────────────────────────────
    BookEntryDef {
        species: &species::MYOTIS_YANBARENSIS,
        commonness: Some(Commonness::Endangered),
        description: Some("FM calls above 40 kHz. Endemic to northern Okinawa (Yanbaru forest). Described in 1997. One of the rarest bats in Japan. Subtropical forest only."),
        name: None,
    },
    BookEntryDef {
        species: &species::MURINA_TENEBROSA,
        commonness: Some(Commonness::Endangered),
        description: Some("FM calls presumed similar to other Murina. Known only from a single holotype on Tsushima Island (1962). Possibly extinct due to deforestation. Alliance for Zero Extinction species."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_STURDEEI,
        commonness: Some(Commonness::Endangered),
        description: Some("Presumed FM-QCF calls. Known only from a specimen on the Bonin Islands (Ogasawara). Not seen since 1915\u{2014}possibly extinct."),
        name: None,
    },

    // ── Non-echolocating ─────────────────────────────────────────
    BookEntryDef {
        species: &species::PTEROPUS_DASYMALLUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Large fruit bat of the Ryukyu Islands (4 subspecies in Japan). Does not echolocate. Feeds on fruit, flowers, and leaves. Endangered due to habitat loss and hunting."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// United Kingdom — all 18 resident species + vagrants
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Russ (2012): British Bat Calls — A Guide to Species Identification
// - Dietz & Kiefer (2016): Bats of Britain and Europe
// - BCT (Bat Conservation Trust): UK bat species profiles
// - Barlow & Jones (1999): Pipistrellus pipistrellus / pygmaeus cryptic species
// - Jan et al. (2010): First UK record of Myotis alcathoe

const UK_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::PIPISTRELLUS_PIPISTRELLUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Britain's most common bat. Peak frequency ~45 kHz; calls heard on almost every bat detector outing. Roosts in buildings, forages along hedgerows, woodland edges, and over water. Often the first species new bat workers learn to identify."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_PYGMAEUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Split from common pipistrelle in 1999. Peak frequency ~55 kHz. Strongly associated with waterways and riparian habitats across the UK. Maternity roosts can hold over 1,000 individuals. Sometimes called the 55 kHz pipistrelle."),
        name: None,
    },
    BookEntryDef {
        species: &species::PLECOTUS_AURITUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Widespread across Britain. Very quiet, broadband FM calls (25–50 kHz) rarely detected beyond a few metres. Gleaning feeder — picks moths and spiders from foliage. Roosts in older buildings, churches, and tree holes. Ears nearly as long as the body."),
        name: None,
    },
    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::MYOTIS_DAUBENTONII,
        commonness: Some(Commonness::Common),
        description: Some("The 'water bat' — iconic low passes skimming rivers, canals, and lakes, scooping insects from the surface. Regular FM calls sweep from ~85 kHz to ~32 kHz. Roosts under bridges and in tunnels. Widespread across England, Wales, and Scotland."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_NATTERERI,
        commonness: Some(Commonness::Common),
        description: Some("Broadband FM sweeps from ~115 kHz down to ~25 kHz. Gleaner — hunts close to vegetation, sometimes hovering to pick prey off leaves. Distinctive fringe of stiff hairs along the tail membrane. Roosts in old buildings, trees, and bat boxes across Britain."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_MYSTACINUS,
        commonness: Some(Commonness::Common),
        description: Some("Small Myotis, widespread but under-recorded. FM calls sweep ~90–35 kHz, very similar to Brandt's bat on a detector. Reliable separation requires examination in the hand. Roosts in buildings; forages along woodland edges and over water."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_BRANDTII,
        commonness: Some(Commonness::Common),
        description: Some("Cryptic species pair with whiskered bat — only separated in 1970. Calls nearly identical (~90–35 kHz FM). Tends to be more woodland-associated than whiskered. Found across England and Wales; scarcer in Scotland."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTALUS_NOCTULA,
        commonness: Some(Commonness::Common),
        description: Some("Britain's largest common bat. Distinctive loud, narrow-band calls around 20–25 kHz, often alternating with steep FM sweeps. One of the first bats out at dusk — sometimes flies in daylight. Open-air forager, often high above the canopy. Roosts in tree holes."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTALUS_LEISLERI,
        commonness: Some(Commonness::Common),
        description: Some("Smaller cousin of the noctule. Calls at ~25–27 kHz — slightly higher than noctule. Fast, high-flying forager. More common in Ireland than in Great Britain. Roosts in tree holes and bat boxes; rarely in buildings."),
        name: None,
    },
    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::EPTESICUS_SEROTINUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large bat of southern England. Loud FM-QCF calls with characteristic frequency around 25–28 kHz. Slow, heavy flight often along treelines and around street lights. Roosts almost exclusively in buildings. Range rarely extends north of the Midlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_NATHUSII,
        commonness: Some(Commonness::Uncommon),
        description: Some("Long-distance migrant — birds ringed in Latvia have been found in the UK. Peak frequency ~38 kHz; calls can resemble common pip but slightly lower and often with distinctive social calls. Breeding colonies now established in several UK sites. Associated with waterside habitats."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_FERRUMEQUINUM,
        commonness: Some(Commonness::Uncommon),
        description: Some("Britain's largest horseshoe bat. Constant-frequency calls at ~82 kHz — unmistakable warbling tone on a heterodyne detector. Restricted to south-west England and south Wales. Hibernates in caves and mines; summer roosts in large roof spaces. UK population internationally important."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_HIPPOSIDEROS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Tiny horseshoe bat weighing only 5–9 g. CF calls at ~110 kHz. Found in western Britain — south-west England, Wales, and western Ireland. Very sensitive to roost disturbance and light pollution. Hibernates underground; maternity roosts in buildings."),
        name: None,
    },
    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::BARBASTELLA_BARBASTELLUS,
        commonness: Some(Commonness::Rare),
        description: Some("Distinctive flat-faced bat with alternating call frequencies — short FM pulses alternating between ~32 kHz and ~43 kHz. One of the UK's rarest bats. Hunts tympanate moths and can switch frequencies to evade moth hearing. Found in mature woodland in southern England; roosts behind loose bark."),
        name: None,
    },
    BookEntryDef {
        species: &species::PLECOTUS_AUSTRIACUS,
        commonness: Some(Commonness::Rare),
        description: Some("Very similar to brown long-eared but restricted to the southern coast of England, mainly around the Channel Islands border. Slightly louder calls than brown long-eared. Fewer than 1,000 individuals estimated in the UK. Roosts in buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_BECHSTEINII,
        commonness: Some(Commonness::Rare),
        description: Some("Elusive woodland specialist of southern England. Long ears (for a Myotis) and broadband FM calls. Rarely caught in mist nets — stays within the canopy. Associated with ancient woodland with veteran trees. One of the UK's rarest resident bats."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_ALCATHOE,
        commonness: Some(Commonness::Rare),
        description: Some("Europe's smallest Myotis, confirmed in the UK in 2010. FM calls sweep from ~100 kHz to ~40 kHz with peak energy ~50–55 kHz — slightly higher than whiskered or Brandt's. Very few confirmed UK sites; likely under-recorded. Requires DNA or detailed morphometrics for reliable identification."),
        name: None,
    },
    // ── Endangered ───────────────────────────────────────────────
    BookEntryDef {
        species: &species::MYOTIS_MYOTIS,
        commonness: Some(Commonness::Endangered),
        description: Some("Britain's largest Myotis. Declared extinct as a UK breeding species in 1990, but a single male has hibernated in a Sussex mine since at least 2002. Loud FM calls sweep ~25–80 kHz. Gleaner — picks large beetles from the ground. Any UK sighting is nationally significant."),
        name: None,
    },
    // ── Vagrant ──────────────────────────────────────────────────
    BookEntryDef {
        species: &species::VESPERTILIO_MURINUS,
        commonness: Some(Commonness::Vagrant),
        description: Some("Continental migrant occasionally reaching eastern England and North Sea oil rigs, mostly in autumn. Distinctive alternating call pattern at ~24 kHz and ~30 kHz. Loud social calls audible to the human ear."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_KUHLII,
        commonness: Some(Commonness::Vagrant),
        description: Some("Mediterranean species expanding northward through Europe. Peak frequency ~40 kHz. Extremely rare in the UK with only a handful of confirmed records, but range expansion may bring more sightings."),
        name: None,
    },
    BookEntryDef {
        species: &species::HYPSUGO_SAVII,
        commonness: Some(Commonness::Vagrant),
        description: Some("Shallow FM sweep with quasi-constant-frequency tail at ~32–34 kHz. Primarily a Mediterranean species but increasingly recorded crossing the Channel to southern England. Distinctive call shape helps separate it from pipistrelles on a spectrogram."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_NILSSONII,
        commonness: Some(Commonness::Vagrant),
        description: Some("Northern bat of Scandinavia and continental Europe. Calls around 27–30 kHz. Very rare UK vagrant — most records are from Scotland and the Northern Isles."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// North America (USA + Canada) — species-level entries sorted by commonness
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Fenton & Bell (1981), O'Farrell et al. (1999), Szewczak (2004)
// - Holroyd et al. (2014): Canadian bat species
// - Kunz & Fenton (2003): Bat Ecology
// - Harvey et al. (2011): Bats of the United States and Canada
// - Various state acoustic ID guides (SonoBat, Bat Call ID)

const NORTH_AMERICA_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::EPTESICUS_FUSCUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("North America's most frequently encountered bat in buildings. FM-QCF calls with characteristic frequency ~30 kHz are loud and distinctive on spectrograms. Tolerates cold well; one of few species active in winter."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_BRASILIENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Forms the largest bat colonies on Earth — Bracken Cave, TX holds ~20 million. Flexible QCF acoustics: 49–70 kHz in open space, drops to 25–40 kHz near clutter. Long narrow wings for fast, high flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_LUCIFUGUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Historically the most commonly encountered bat across northern North America. FM sweep ~80–40 kHz, characteristic frequency ~45 kHz. Populations have crashed >90% in eastern range due to White-nose Syndrome since 2006."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_BOREALIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Solitary foliage-roosting tree bat with distinctive reddish fur. FM sweep with characteristic frequency ~40 kHz. Long-distance migrant. One of the most common bats in eastern forests and suburbs."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_CINEREUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("North America's largest bat (~30 g) with frosted brown fur. Distinctive low-frequency QCF calls ~20–25 kHz, unmistakable on spectrograms. Solitary, migratory. Most frequently killed bat at wind energy facilities."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIONYCTERIS_NOCTIVAGANS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Medium-sized bat with silver-tipped dark fur. Low-frequency QCF calls ~25–27 kHz. Slow, maneuverable flight over water and forest clearings. Solitary tree-roosting migrant. Second most common wind turbine fatality."),
        name: None,
    },
    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::PERIMYOTIS_SUBFLAVUS,
        commonness: Some(Commonness::Common),
        description: Some("Tiny bat (~6 g) formerly called Eastern Pipistrelle. FM sweep ~40–55 kHz, characteristic frequency ~45 kHz. Distinctive slow, erratic fluttery flight. Tricolored fur bands. Severely impacted by White-nose Syndrome."),
        name: None,
    },
    BookEntryDef {
        species: &species::PARASTRELLUS_HESPERUS,
        commonness: Some(Commonness::Common),
        description: Some("Smallest North American bat (~3.5 g), formerly Western Pipistrelle. FM sweep with characteristic frequency ~50 kHz. Common in desert canyons. Often the first bat flying at dusk, sometimes before sunset."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTICEIUS_HUMERALIS,
        commonness: Some(Commonness::Common),
        description: Some("Medium-sized bat of the eastern US. FM-QCF calls ~35 kHz characteristic frequency. Resembles a small Big Brown Bat. Roosts in tree cavities and buildings; does not use caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_CALIFORNICUS,
        commonness: Some(Commonness::Common),
        description: Some("Small, adaptable western Myotis. FM sweep with characteristic frequency ~50 kHz. Often the most common bat detected at western sites. Difficult to distinguish acoustically from Western Small-footed Myotis."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_YUMANENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Western water-foraging specialist that trawls insects from the surface with oversized feet. FM sweep with characteristic frequency ~50 kHz. Common near rivers, lakes, and stock tanks."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_VOLANS,
        commonness: Some(Commonness::Common),
        description: Some("Western coniferous forest bat. FM sweep with characteristic frequency ~40 kHz. Keeled calcar and furred underwing membrane are diagnostic in hand. Fast, direct flight through forest canopy."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_VELIFER,
        commonness: Some(Commonness::Common),
        description: Some("Large Myotis of the south-central US. FM sweep with characteristic frequency ~45 kHz. Forms large cave colonies. Bare patches on back between shoulder blades are diagnostic in hand."),
        name: None,
    },
    BookEntryDef {
        species: &species::ANTROZOUS_PALLIDUS,
        commonness: Some(Commonness::Common),
        description: Some("Unique dual forager: echolocates for aerial prey and passively listens to glean scorpions and large insects from the ground. FM calls ~30 kHz. Large ears, pale fur. Immune to scorpion venom. Western deserts and grasslands."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_SEMINOLUS,
        commonness: Some(Commonness::Common),
        description: Some("Southeastern counterpart of the Eastern Red Bat with deep mahogany fur. FM sweep ~35–50 kHz, characteristic frequency ~40 kHz. Roosts in Spanish moss and pine needle clusters."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_BLOSSEVILLII,
        commonness: Some(Commonness::Common),
        description: Some("Western counterpart of the Eastern Red Bat. FM-QCF calls with characteristic frequency ~42 kHz. Solitary foliage-roosting migrant. Found from British Columbia to Argentina. Reddish fur similar to Eastern Red Bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::CORYNORHINUS_TOWNSENDII,
        commonness: Some(Commonness::Common),
        description: Some("Enormous ears (~35 mm). Very quiet, short FM calls ~25–40 kHz for gleaning moths. Highly sensitive to roost disturbance. Found across the western US; several isolated eastern subspecies are endangered."),
        name: None,
    },
    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::MYOTIS_THYSANODES,
        commonness: Some(Commonness::Uncommon),
        description: Some("Named for the distinctive fringe of stiff hairs along the tail membrane. FM sweep ~25–55 kHz, characteristic frequency ~40 kHz. Western mountains; roosts in caves, mines, and buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_EVOTIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Very quiet, short FM calls typical of a gleaning forager, characteristic frequency ~40 kHz. Large ears extend well beyond the nose when laid forward. Western forests and woodlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_CILIOLABRUM,
        commonness: Some(Commonness::Uncommon),
        description: Some("Tiny bat (~5 g) of western arid lands. FM sweep with characteristic frequency ~50 kHz. Calls nearly identical to California Myotis. Roosts in rock crevices, cliff faces, and eroded badlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_LEIBII,
        commonness: Some(Commonness::Uncommon),
        description: Some("One of North America's smallest bats (~5 g). FM sweep with characteristic frequency ~50–55 kHz. Roosts in rock crevices and talus slopes. Appears somewhat resistant to White-nose Syndrome."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_AUSTRORIPARIUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Southeastern US cave bat often found near water. FM sweep with characteristic frequency ~50 kHz. Forms large maternity colonies; Florida cave colonies can number in the tens of thousands."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_INTERMEDIUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large yellowish tree bat of the southeastern coastal plain. FM-QCF calls ~30 kHz characteristic frequency. Roosts in dead palm fronds and Spanish moss along the Gulf and Atlantic coasts."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_XANTHINUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Southwestern desert species with yellowish fur. FM-QCF calls ~30 kHz characteristic frequency. Roosts in palm fronds. Range expanding northward with ornamental palm plantings in urban areas."),
        name: None,
    },
    BookEntryDef {
        species: &species::CORYNORHINUS_RAFINESQUII,
        commonness: Some(Commonness::Uncommon),
        description: Some("Southeastern big-eared bat with white belly fur. Very quiet gleaning calls ~25–40 kHz. Roosts in abandoned buildings, hollow trees, and under bridges. State-listed in many southeastern states."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTINOMOPS_MACROTIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large free-tailed bat with low QCF calls ~14–17 kHz, often audible to humans. Roosts in high cliff crevices in the Southwest. Loud piercing social calls carry long distances."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTINOMOPS_FEMOROSACCUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Medium-sized free-tailed bat of the southwestern deserts. QCF calls ~22 kHz. Named for the pocket-like fold on the tail membrane. Roosts in rock crevices in desert canyon country."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_PEROTIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Largest bat in North America by wingspan (~56 cm). Very low QCF calls ~10–12 kHz, easily audible. Needs a vertical drop to launch into flight; roosts on tall cliff faces and high buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MACROTUS_CALIFORNICUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Low-intensity gleaning FM calls ~50 kHz, hard to detect. Large ears and prominent nose leaf. Non-migratory desert species in AZ and southern CA. Cannot hibernate; depends on warm mines and caves year-round."),
        name: None,
    },
    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::EUDERMA_MACULATUM,
        commonness: Some(Commonness::Rare),
        description: Some("Lowest echolocation frequency of any North American bat (~10–12 kHz), easily audible. Unmistakable appearance: three large white spots on jet-black fur, enormous pink ears. Sparse but widespread across western arid lands."),
        name: None,
    },
    BookEntryDef {
        species: &species::IDIONYCTERIS_PHYLLOTIS,
        commonness: Some(Commonness::Rare),
        description: Some("Low-frequency calls ~12–15 kHz, often audible. Distinctive lappets project from base of oversized ears. Southwestern pine-oak forests. Calls unlike any other North American bat of similar size."),
        name: None,
    },
    BookEntryDef {
        species: &species::MORMOOPS_MEGALOPHYLLA,
        commonness: Some(Commonness::Rare),
        description: Some("Range barely enters southern Texas at a single cave. CF-FM calls ~53–63 kHz. Bizarre facial features with leaf-like skin flaps. Tropical species at the extreme northern edge of its range in the US."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHOERONYCTERIS_MEXICANA,
        commonness: Some(Commonness::Rare),
        description: Some("Nectar-feeding bat with elongated snout. Very quiet FM calls ~75 kHz. Seasonal migrant to southern Arizona. Visits hummingbird feeders. Low-intensity echolocation typical of phyllostomids."),
        name: None,
    },
    BookEntryDef {
        species: &species::LEPTONYCTERIS_YERBABUENAE,
        commonness: Some(Commonness::Rare),
        description: Some("Major pollinator of saguaro and organ pipe cacti. Quiet FM calls ~75 kHz. Migrates to southern Arizona in summer. Formerly endangered, delisted in 2018 — a conservation success story."),
        name: None,
    },
    // ── Endangered ───────────────────────────────────────────────
    BookEntryDef {
        species: &species::MYOTIS_SEPTENTRIONALIS,
        commonness: Some(Commonness::Endangered),
        description: Some("Steep broadband FM sweeps ~60–115 kHz. Gleaning forager in forest understory. Populations have declined >99% in parts of the eastern range due to White-nose Syndrome. Federally endangered since 2023."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_SODALIS,
        commonness: Some(Commonness::Endangered),
        description: Some("Calls nearly identical to Little Brown Myotis, characteristic frequency ~45 kHz. Hibernates in dense clusters in limestone caves; a single cave may hold thousands. Federally endangered since 1967."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_GRISESCENS,
        commonness: Some(Commonness::Endangered),
        description: Some("Largest eastern Myotis. FM sweep ~70–35 kHz, characteristic frequency ~45 kHz. Obligate cave bat year-round. Populations recovering under federal protection; some colonies now exceed historic numbers."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_FLORIDANUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Restricted to southern Florida. Low QCF calls ~14–16 kHz, occasionally audible. One of the rarest bats in North America. Roosts in tree cavities and bat houses. Federally endangered."),
        name: None,
    },
    BookEntryDef {
        species: &species::LEPTONYCTERIS_NIVALIS,
        commonness: Some(Commonness::Endangered),
        description: Some("Nectar bat that migrates to the Big Bend region of Texas in summer. Quiet FM calls ~75 kHz. Pollinates agave. Only known US roost is a single cave in the Chisos Mountains. Federally endangered."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// South America — species-level entries sorted by commonness
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

const SOUTH_AMERICA_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::SACCOPTERYX_BILINEATA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant across Amazonian and Atlantic Forest lowlands. Roosts on tree trunks and building walls. Alternates ~45/48 kHz. Complex vocal learning with regional song dialects."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_MOLOSSUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant in buildings and urban areas throughout South America. Alternating QCF at ~34.5/39.6 kHz. One of the first bats detected at dusk. Open-space aerial hawker."),
        name: None,
    },
    BookEntryDef {
        species: &species::CAROLLIA_PERSPICILLATA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("One of the most abundant South American bats. Peak ~71 kHz. Low-intensity whispering calls\u{2014}detectable only within a few meters. Key seed disperser of Piper plants."),
        name: None,
    },
    BookEntryDef {
        species: &species::ARTIBEUS_LITURATUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant large frugivore throughout South America. Lower peak (~52 kHz) than congeners. Prominent facial stripes. Low-intensity multiharmonic FM. Important seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::ARTIBEUS_PLANIROSTRIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Very common frugivore, replaces A. jamaicensis across much of mainland South America. Peak ~54 kHz. Low-intensity multiharmonic FM. Important seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLOSSOPHAGA_SORICINA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant nectarivore throughout South America. Peak ~80 kHz. Low-intensity calls\u{2014}hard to detect beyond a few meters. Uses echolocation to find flowers with acoustic reflectors. Important pollinator."),
        name: None,
    },
    BookEntryDef {
        species: &species::STURNIRA_LILIUM,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Very common frugivore across South America. Peak ~66.5 kHz. Low-intensity FM. Yellow shoulder epaulettes in males. Key seed disperser of Solanum."),
        name: None,
    },
    BookEntryDef {
        species: &species::DESMODUS_ROTUNDUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Common throughout South America near livestock. Peak ~55 kHz. Relatively long calls for a phyllostomid (~5.5 ms). Low-intensity. Obligate blood-feeder. Important rabies vector in livestock regions."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_BRASILIENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant throughout South America\u{2014}the type locality is in Brazil. Extremely flexible acoustics: QCF 49\u{2013}70 kHz in open space, drops to 25\u{2013}40 kHz near objects. Forms massive colonies. Fast, high-altitude forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_RUFUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Widespread in South American lowlands. Low-frequency QCF at ~25\u{2013}26 kHz. Largest Molossus. Roosts in buildings and hollow trees. Open-space forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_NIGRICANS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("One of the most widespread South American vespertilionids. Highly plastic calls adapting to clutter. Peak ~50 kHz. FM sweeps. Common in forests and urban edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::PLATYRRHINUS_LINEATUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Common frugivore of open habitats and forest edges. Peak ~62 kHz. Prominent white facial and dorsal stripes. Low-intensity FM. Cerrado and Atlantic Forest."),
        name: None,
    },

    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::PTERONOTUS_PARNELLII,
        commonness: Some(Commonness::Common),
        description: Some("The South American high-duty-cycle echolocator. Long CF at ~60 kHz (2nd harmonic) with Doppler compensation. Unmistakable call structure. Huge cave colonies. Split from P. mesoamericanus."),
        name: None,
    },
    BookEntryDef {
        species: &species::SACCOPTERYX_LEPTURA,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in Amazonian lowland forests. Higher frequency (~50 kHz) than S. bilineata. Similar QCF structure. Thinner dorsal stripes. Roosts on tree trunks."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHYNCHONYCTERIS_NASO,
        commonness: Some(Commonness::Common),
        description: Some("Common along Amazonian rivers, often in lines under bridges and overhanging banks. CF-FM with peak at ~47 kHz. Cryptic bark-like camouflage. Widespread from Mexico to Bolivia."),
        name: None,
    },
    BookEntryDef {
        species: &species::NOCTILIO_LEPORINUS,
        commonness: Some(Commonness::Common),
        description: Some("Common along South American rivers and coasts. Long CF at 53\u{2013}56 kHz + FM sweep. Rakes water with large clawed feet to catch fish. Found throughout the continent."),
        name: None,
    },
    BookEntryDef {
        species: &species::NOCTILIO_ALBIVENTRIS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread along Amazonian waterways. Higher CF (~75 kHz) than N. leporinus. Trawls insects and small fish from water surfaces."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_GYMNONOTUS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in South American lowlands. CF at ~54\u{2013}57 kHz. Largest Pteronotus. Bare-backed in flight. Cave-dwelling. Often in mixed colonies with other mormoopids."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_PERSONATUS,
        commonness: Some(Commonness::Common),
        description: Some("South American lowlands. Highest frequency Pteronotus: initial CF ~83 kHz, terminal ~68 kHz. Cave-roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::PEROPTERYX_MACROTIS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in South American lowlands. Multiharmonic QCF at ~40 kHz (2nd harmonic). Found near caves and rock shelters."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHYLLOSTOMUS_HASTATUS,
        commonness: Some(Commonness::Common),
        description: Some("Common in South American lowlands. Large omnivore. Peak ~47 kHz. Low-intensity FM. Harem groups in caves and hollow trees. One of the loudest phyllostomids."),
        name: None,
    },
    BookEntryDef {
        species: &species::TRACHOPS_CIRRHOSUS,
        commonness: Some(Commonness::Common),
        description: Some("Common in South American lowland forests. Famous frog-eating bat\u{2014}identifies prey by their mating calls. Peak ~70 kHz. Low-intensity FM. Warty lips."),
        name: None,
    },
    BookEntryDef {
        species: &species::CAROLLIA_BREVICAUDA,
        commonness: Some(Commonness::Common),
        description: Some("Andean foothills and premontane forests throughout South America. Intermediate peak (~73 kHz) between C. perspicillata and C. castanea. Frugivore specializing on Piper."),
        name: None,
    },
    BookEntryDef {
        species: &species::ARTIBEUS_OBSCURUS,
        commonness: Some(Commonness::Common),
        description: Some("Common Amazonian frugivore. Peak ~55 kHz. Intermediate size between A. jamaicensis and A. lituratus. Low-intensity FM. Indistinct facial stripes."),
        name: None,
    },
    BookEntryDef {
        species: &species::URODERMA_BILOBATUM,
        commonness: Some(Commonness::Common),
        description: Some("Tent-roosting frugivore in northern South American lowlands. Peak ~70 kHz. Low-intensity nasal FM. Modifies palm and banana leaves into tents."),
        name: None,
    },
    BookEntryDef {
        species: &species::DERMANURA_PHAEOTIS,
        commonness: Some(Commonness::Common),
        description: Some("Small frugivore of northern SA lowlands. Peak ~75 kHz. Low-intensity FM. Tent-roosting. Important disperser of understory fruits."),
        name: None,
    },
    BookEntryDef {
        species: &species::LOPHOSTOMA_SILVICOLUM,
        commonness: Some(Commonness::Common),
        description: Some("Common in Amazonian forests. Modifies active termite nests into roosts\u{2014}unique among bats. Peak ~70 kHz. Low-intensity FM. Gleaning insectivore."),
        name: None,
    },
    BookEntryDef {
        species: &species::MICRONYCTERIS_MICROTIS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in South American forests. Very short broadband FM at ~90\u{2013}100 kHz. Ultra-low intensity. Gleaning insectivore with large ears."),
        name: None,
    },
    BookEntryDef {
        species: &species::ANOURA_GEOFFROYI,
        commonness: Some(Commonness::Common),
        description: Some("Andean cloud forests and highlands throughout South America. Nectarivore. Peak ~70 kHz. Low-intensity FM. Important pollinator. Tailless."),
        name: None,
    },
    BookEntryDef {
        species: &species::ANOURA_CAUDIFER,
        commonness: Some(Commonness::Common),
        description: Some("Nectarivore with short tail (unlike tailless A. geoffroyi). Peak ~72 kHz. Low-intensity FM. Atlantic Forest and lower Andean slopes. Important pollinator."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_AURIPENDULUS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in South American lowlands. Alternating QCF at ~23\u{2013}26 kHz. High, fast flight above canopy. Large ears joined at base."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_SINALOAE,
        commonness: Some(Commonness::Common),
        description: Some("Northern South America (Colombia, Venezuela, Ecuador). QCF at ~34 kHz. Open-space forager. Roosts in buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSOPS_TEMMINCKII,
        commonness: Some(Commonness::Common),
        description: Some("Small but widespread molossid. QCF at ~38\u{2013}42 kHz\u{2014}one of the highest frequency molossids, consistent with small body size. Open cerrado and forest edge forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_BRASILIENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Common across South American lowlands and mid-elevations. Peak ~54\u{2013}60 kHz. FM-QCF. Forest edges and open areas."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_BLOSSEVILLII,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in South America. Open-air forager. Peak ~42 kHz. FM-QCF. Migratory. Distinctive reddish fur. Roosts solitarily in foliage."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_ALBESCENS,
        commonness: Some(Commonness::Common),
        description: Some("Common South American Myotis. Peak ~48 kHz. FM sweeps. Silver-tipped dorsal fur distinctive. Forages over water and in forest clearings. Widespread from Mexico to Argentina."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_CHILOENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Southern South American Myotis (Chile, Argentina, Patagonia). Peak ~45 kHz. FM sweeps. One of the southernmost-ranging bats in the Americas. Forest and edge habitats."),
        name: None,
    },

    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::CORMURA_BREVIROSTRIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Amazonian forest-interior forager. Unusual: most energy in 5th harmonic at ~68 kHz. Multiharmonic calls. Roosts in hollow trees."),
        name: None,
    },
    BookEntryDef {
        species: &species::PEROPTERYX_KAPPLERI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Amazonian lowlands. Lower frequency (~32 kHz) than P. macrotis. 2nd harmonic dominant. Near caves and rocky outcrops."),
        name: None,
    },
    BookEntryDef {
        species: &species::SACCOPTERYX_CANESCENS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Amazonian endemic. Highest frequency Saccopteryx (~52 kHz). Similar QCF structure to congeners. Smaller than S. bilineata. Roosts on tree trunks in terra firme forest."),
        name: None,
    },
    BookEntryDef {
        species: &species::CENTRONYCTERIS_MAXIMILIANI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Rare canopy-level forager. Steep FM sweeps ~45\u{2013}55 kHz. Long shaggy fur. One of the least-known Neotropical emballonurids. Amazonian forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::PEROPTERYX_LEUCOPTERA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Distinctive white wing membrane patches. QCF at ~42 kHz. Amazonian lowland forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTERONOTUS_DAVYI,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American populations in Venezuela, Colombia, Peru. CF-FM at ~67 kHz with sweep to ~51 kHz. Cave-roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::MORMOOPS_MEGALOPHYLLA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Northern South America (Venezuela, Colombia). 2nd harmonic at ~67 kHz dominates. Large cave colonies. At southern edge of range."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHYLLOSTOMUS_DISCOLOR,
        commonness: Some(Commonness::Uncommon),
        description: Some("Widespread in South American lowlands. Omnivore. Peak ~55 kHz. Low-intensity FM. Large colonies in hollow trees. Important pollinator of balsa trees."),
        name: None,
    },
    BookEntryDef {
        species: &species::LONCHORHINA_AURITA,
        commonness: Some(Commonness::Uncommon),
        description: Some("UNIQUE phyllostomid with CF-FM calls\u{2014}the only leaf-nosed bat with constant-frequency echolocation. Long CF at ~45 kHz. Longest phyllostomid calls (up to 8.7 ms). Amazonian caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::MIMON_CRENULATUM,
        commonness: Some(Commonness::Uncommon),
        description: Some("Widespread in South American forests. Gleaning insectivore. Peak ~75 kHz. Low-intensity FM. Now Gardnerycteris crenulatum. Forest understory."),
        name: None,
    },
    BookEntryDef {
        species: &species::TONATIA_SAUROPHILA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Amazonian forests. Gleaning insectivore/carnivore. Peak ~65 kHz. Low-intensity FM. Roosts in hollow trees. Forest interior specialist."),
        name: None,
    },
    BookEntryDef {
        species: &species::CAROLLIA_CASTANEA,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American lowland forests. Higher peak (~78 kHz) than C. perspicillata. Low-intensity FM. Smallest Carollia."),
        name: None,
    },
    BookEntryDef {
        species: &species::PLATYRRHINUS_HELLERI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small frugivore. Peak ~68 kHz. Low-intensity FM. Widespread in Neotropical lowland forests. Important seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::VAMPYRESSA_THYONE,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small frugivore. Peak ~78 kHz. Low-intensity FM. Tent-roosting. Yellow ear margins. Northern South American lowlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::VAMPYRODES_CARACCIOLI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large stenodermatine frugivore. Peak ~58 kHz. Prominent white facial stripes. Low-intensity FM. Forages in canopy. Amazonian and northern SA lowlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHIRODERMA_VILLOSUM,
        commonness: Some(Commonness::Uncommon),
        description: Some("Canopy frugivore with large eyes. Peak ~62 kHz. Low-intensity FM. Widespread in Neotropical lowlands. White dorsal stripe."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOPHYLLA_PUMILIO,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small Amazonian frugivore. Peak ~80 kHz. Low-intensity FM. Related to Carollia. Important disperser of understory fruits. Common in terra firme forest."),
        name: None,
    },
    BookEntryDef {
        species: &species::LONCHOPHYLLA_THOMASI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small nectarivore. Peak ~80 kHz. Low-intensity multiharmonic FM. Important pollinator of understory plants. Amazonian forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::STURNIRA_ERYTHROMOS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Andean frugivore of montane forests (1000\u{2013}3200 m). Peak ~70 kHz. Low-intensity FM. Key seed disperser in cloud forest ecosystems."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTINOMOPS_LATICAUDATUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Widespread in South America. Distinctive three-frequency alternation (~26.7, 28.7, 32.4 kHz). Open-space forager. Cliff and building roosts."),
        name: None,
    },
    BookEntryDef {
        species: &species::CYNOMOPS_GREENHALLI,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American lowlands. Low frequency (~22 kHz) open-space forager. Roosts in hollow trees and buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::PROMOPS_CENTRALIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American lowlands. Distinctive upward-modulated QCF. Alternating pairs at ~30/35 kHz. Open-space forager above canopy."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_GLAUCINUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American lowlands. Very low frequency (~22\u{2013}25 kHz) QCF. High-altitude forager above canopy. Large bonneted bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_BONARIENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Medium-sized bonneted bat. QCF at ~26\u{2013}30 kHz. Open-area forager. Southern South America (Argentina, Uruguay, Brazil). Roosts in buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_PATAGONICUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Medium bonneted bat of the southern cone (Argentina, Paraguay, southern Brazil). QCF at ~28 kHz. Open and semi-open habitat forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::PROMOPS_NASUTUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("QCF at ~30 kHz. Similar to P. centralis but lacks the distinctive upward frequency modulation. Open-space forager. South American drylands and forest edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::CYNOMOPS_ABRASUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Medium molossid. Low-frequency QCF at ~22\u{2013}24 kHz. Open-space forager. Brazilian cerrado and Atlantic Forest edge."),
        name: None,
    },
    BookEntryDef {
        species: &species::CYNOMOPS_PLANIROSTRIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small to medium molossid. QCF at ~25 kHz. Flat face with forward-pointing nostrils. Open habitats across South American lowlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_KEAYSI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Andean highlands and cloud forests. Short FM calls (~2.5 ms). Peak ~55 kHz. Forages in cluttered forest understory."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_RIPARIUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Widespread in South American lowlands. Steep broadband FM sweep from ~120 to ~50 kHz. Forages near streams and over water."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_FURINALIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American lowlands. Lower frequency (~43 kHz) than E. brasiliensis. FM-QCF. Forest edges and open areas."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_LEVIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("South American Myotis. Peak ~46 kHz. FM sweeps. Southern Brazil, Uruguay, Argentina. Open areas and forest edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_OXYOTUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("High-altitude Andean Myotis (1500\u{2013}3800 m). Peak ~50 kHz. FM sweeps. Cloud forests and paramo edges. Colombia to Bolivia."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_VILLOSISSIMUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large vespertilionid, recently split from L. cinereus. Low-frequency FM-QCF at ~22\u{2013}25 kHz. Long narrow wings for fast open-air flight. Migratory. Frosted fur."),
        name: None,
    },
    BookEntryDef {
        species: &species::HISTIOTUS_MONTANUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Distinctive very large ears (Plecotus-like). Peak ~28 kHz. FM sweeps. Open habitats in southern South America (Patagonia to southern Brazil). Slow, maneuverable flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::HISTIOTUS_VELATUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large-eared vespertilionid. Peak ~32 kHz. FM sweeps. Brazilian cerrado and Atlantic Forest. Slightly higher frequency than H. montanus, consistent with smaller ears."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_DIMINUTUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small Eptesicus of southern Brazil, Paraguay, Argentina. Peak ~50 kHz. FM-QCF. Forest edges and open areas."),
        name: None,
    },
    BookEntryDef {
        species: &species::LASIURUS_EGA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Widespread in South American lowlands. Lower peak (~35 kHz) than L. blossevillii. FM-QCF. Roosts solitarily in palm fronds. Yellow fur."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_BONDAE,
        commonness: Some(Commonness::Uncommon),
        description: Some("Northern South America (Colombia, Venezuela, Ecuador). QCF at ~33 kHz. Open-space forager. Roosts in buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSUS_COIBENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Northern South America. QCF at ~35 kHz. Smallest Molossus. Open-space forager."),
        name: None,
    },

    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::DICLIDURUS_ALBUS,
        commonness: Some(Commonness::Rare),
        description: Some("Rare in Amazonian canopy. Distinctive all-white fur\u{2014}the ghost bat. Narrowband QCF at ~24 kHz. High-altitude forager above the canopy."),
        name: None,
    },
    BookEntryDef {
        species: &species::PEROPTERYX_TRINITATIS,
        commonness: Some(Commonness::Rare),
        description: Some("Northern South America (Venezuela, Trinidad). QCF at ~43 kHz. Open-area forager near rock shelters and caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::BALANTIOPTERYX_PLICATA,
        commonness: Some(Commonness::Rare),
        description: Some("At the southern edge of its range in northern South America (Venezuela, Colombia). Long QCF at ~43 kHz. Open-area forager near caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::DIAEMUS_YOUNGI,
        commonness: Some(Commonness::Rare),
        description: Some("Feeds on bird blood. Peak ~70 kHz. White wing tips distinctive in flight. Low-intensity FM. Rarer than D. rotundus. Lowland forests of South America."),
        name: None,
    },
    BookEntryDef {
        species: &species::DIPHYLLA_ECAUDATA,
        commonness: Some(Commonness::Rare),
        description: Some("Bird blood specialist (unlike the mammal-feeding D. rotundus). Peak ~80 kHz. Higher frequency than common vampire. Short multiharmonic FM. Low-intensity. Atlantic Forest and Amazonia."),
        name: None,
    },
    BookEntryDef {
        species: &species::VAMPYRUM_SPECTRUM,
        commonness: Some(Commonness::Rare),
        description: Some("Largest bat in the Americas (wingspan ~1 m). Peak ~70 kHz. Low-intensity FM\u{2014}very difficult to detect acoustically. Carnivorous: preys on birds and other bats."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHROTOPTERUS_AURITUS,
        commonness: Some(Commonness::Rare),
        description: Some("Carnivorous gleaner. Peak ~77 kHz. Short FM (0.8\u{2013}1.4 ms). Low-intensity\u{2014}hard to detect beyond a few meters. Large ears; hunts other bats and rodents."),
        name: None,
    },
    BookEntryDef {
        species: &species::MACROPHYLLUM_MACROPHYLLUM,
        commonness: Some(Commonness::Rare),
        description: Some("Unusual trawling phyllostomid. Louder than most relatives (~101 dB SPL). Peak ~85 kHz. Large feet for grabbing insects from water surfaces."),
        name: None,
    },
    BookEntryDef {
        species: &species::MICRONYCTERIS_HIRSUTA,
        commonness: Some(Commonness::Rare),
        description: Some("Gleaning insectivore. Lower peak (~52 kHz) than M. microtis. Low-intensity FM. Forest interior. Amazonian forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::LAMPRONYCTERIS_BRACHYOTIS,
        commonness: Some(Commonness::Rare),
        description: Some("Rare gleaning insectivore. Peak ~75 kHz. Low-intensity FM. Poorly documented acoustically. Yellow throat patches. Amazonian forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLYPHONYCTERIS_SYLVESTRIS,
        commonness: Some(Commonness::Rare),
        description: Some("Rare gleaner. Peak ~85 kHz. Ultra-short broadband FM (0.3\u{2013}1 ms). Very low intensity. Forest interior."),
        name: None,
    },
    BookEntryDef {
        species: &species::TRINYCTERIS_NICEFORI,
        commonness: Some(Commonness::Rare),
        description: Some("Low-intensity gleaner. Peak ~80 kHz. Multiharmonic FM. Forest understory specialist. Rarely captured or detected."),
        name: None,
    },
    BookEntryDef {
        species: &species::MESOPHYLLA_MACCONNELLI,
        commonness: Some(Commonness::Rare),
        description: Some("Tiny Amazonian tent-roosting frugivore. Highest peak frequency of any phyllostomid (~100\u{2013}118 kHz). Ultra-low intensity."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHYLLODERMA_STENOPS,
        commonness: Some(Commonness::Rare),
        description: Some("Large omnivorous phyllostomid. Peak ~55 kHz. Low-intensity FM. Pale face distinctive. Roosts in hollow trees. Amazonian forests."),
        name: None,
    },
    BookEntryDef {
        species: &species::ANOURA_CULTRATA,
        commonness: Some(Commonness::Rare),
        description: Some("Highland nectarivore. Peak ~68 kHz. Low-intensity FM. Andean cloud forests 1000\u{2013}2600 m. Uniquely keeled lower incisors."),
        name: None,
    },
    BookEntryDef {
        species: &species::STURNIRA_BOGOTENSIS,
        commonness: Some(Commonness::Rare),
        description: Some("High-altitude Andean frugivore (2000\u{2013}3400 m). Peak ~68 kHz. Low-intensity FM. One of the few bat species found above 3000 m."),
        name: None,
    },
    BookEntryDef {
        species: &species::THYROPTERA_TRICOLOR,
        commonness: Some(Commonness::Rare),
        description: Some("Suction-cup disks for roosting in rolled Heliconia/Calathea leaves. Extremely low intensity\u{2014}barely detectable at <1 m. Distinctive social calls for roost-finding. Amazonian lowlands."),
        name: None,
    },
    BookEntryDef {
        species: &species::THYROPTERA_DISCIFERA,
        commonness: Some(Commonness::Rare),
        description: Some("Similar to T. tricolor but slightly larger suction disks. Peak ~50 kHz. Roosts in furled Heliconia leaves. Amazonian lowland forests. Extremely low-intensity echolocation."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_HANSAE,
        commonness: Some(Commonness::Rare),
        description: Some("Medium molossid. QCF at ~28\u{2013}32 kHz. Amazonian and Atlantic Forest lowlands. Roosts in tree hollows. Uncommonly recorded."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTINOMOPS_MACROTIS,
        commonness: Some(Commonness::Rare),
        description: Some("Large free-tailed bat. Low-frequency QCF at ~14\u{2013}17 kHz, often audible. Roosts in cliff crevices and buildings. South American drylands and cerrado."),
        name: None,
    },
    BookEntryDef {
        species: &species::ARTIBEUS_JAMAICENSIS,
        commonness: Some(Commonness::Rare),
        description: Some("At the edge of its range in northern coastal South America. Peak ~56 kHz. Low-intensity FM. Important fig seed disperser where it occurs."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHOGEESSA_TUMIDA,
        commonness: Some(Commonness::Rare),
        description: Some("Northern South America. Small vespertilionid. Broadband FM + QCF at ~48 kHz. Forest edges."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_LAVALI,
        commonness: Some(Commonness::Rare),
        description: Some("Small Myotis endemic to eastern Brazil (Cerrado, Caatinga). Peak ~52 kHz. Steep FM sweeps. Recently described. Associated with rock outcrops."),
        name: None,
    },
    BookEntryDef {
        species: &species::FURIPTERUS_HORRENS,
        commonness: Some(Commonness::Rare),
        description: Some("Tiny (3\u{2013}5 g) insectivore with vestigial thumb. Very high frequency FM sweeps peaking ~130\u{2013}150 kHz. One of the highest-frequency New World bats. Caves and mines. Endemic family Furipteridae."),
        name: None,
    },

    // ── Endangered ───────────────────────────────────────────────
    BookEntryDef {
        species: &species::AMORPHOCHILUS_SCHNABLII,
        commonness: Some(Commonness::Endangered),
        description: Some("Rare, endemic to western South America (Ecuador, Peru, Chile). High-frequency FM sweeps ~80\u{2013}100 kHz. Cave-dwelling. One of only two species in the endemic family Furipteridae. IUCN Vulnerable."),
        name: None,
    },
    BookEntryDef {
        species: &species::NATALUS_MACROURUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Very high frequency echolocator, peak ~100\u{2013}120 kHz. Extremely low intensity. Cave-roosting. Endemic to eastern Brazil (Cerrado/Caatinga). IUCN Vulnerable. Cave-dependent."),
        name: None,
    },
    BookEntryDef {
        species: &species::NATALUS_TUMIDIROSTRIS,
        commonness: Some(Commonness::Endangered),
        description: Some("Very high frequency FM, peak ~100\u{2013}130 kHz. Cave-roosting. Northern South America (Venezuela, Colombia, Trinidad). Low-intensity calls barely detectable beyond 1 m. Restricted range."),
        name: None,
    },
    BookEntryDef {
        species: &species::THYROPTERA_LAVALI,
        commonness: Some(Commonness::Endangered),
        description: Some("Rare Amazonian disk-winged bat. Roosts in curled leaves. Poorly known acoustically. Low-intensity FM calls. Restricted range."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUMOPS_DABBENEI,
        commonness: Some(Commonness::Endangered),
        description: Some("Very large molossid. Low-frequency QCF at ~18\u{2013}20 kHz, sometimes audible. Open-space forager over savanna and cerrado. Rarely recorded. Northern Argentina to Colombia."),
        name: None,
    },
    BookEntryDef {
        species: &species::HISTIOTUS_MACROTUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Largest-eared Histiotus. Very low-frequency peak ~25 kHz. FM sweeps. Chile and western Argentina. Arid and semi-arid open habitats. Restricted range."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOLOSSOPS_NEGLECTUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Small Amazonian molossid. QCF at ~40 kHz. Poorly known\u{2014}one of the least-studied South American bats. Forest edge and gap forager."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// Africa — species-level entries sorted by commonness
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Monadjem et al. (2017, 2020), Taylor et al. (2012), Jacobs et al. (2007–2017)
// - Webala et al. (2019), Happold & Happold (2013), ACR

const AFRICA_BOOK: &[BookEntryDef] = &[
    // ── Very Common ──────────────────────────────────────────────
    BookEntryDef {
        species: &species::SCOTOPHILUS_DINGANII,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Most common vespertilionid in sub-Saharan Africa. Two cryptic phonic forms with peaks at ~33 kHz and ~44 kHz. Hockey-stick FM-QCF call shape. Roosts in roofs and buildings across a wide range of habitats."),
        name: None,
    },
    BookEntryDef {
        species: &species::NEOROMICIA_CAPENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Ubiquitous across southern Africa. Peak frequency ~38 kHz. FM-QCF hockey-stick calls. Roosts in buildings. One of the most frequently recorded species on bat detectors in South Africa."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHAEREPHON_PUMILUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Extremely common across sub-Saharan Africa. Narrowband QCF calls peaking ~25 kHz. Forms large colonies in roofs of buildings. Geographic variation in call frequency documented."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_AEGYPTIACA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Throughout Africa and into the Middle East. Long narrowband QCF calls peaking ~18\u{2013}23 kHz. High-flying open-air forager. Very loud calls detectable at long range."),
        name: None,
    },
    BookEntryDef {
        species: &species::HIPPOSIDEROS_CAFFER,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Most widespread African hipposiderid. High CF calls (peak ~132\u{2013}141 kHz). Frequency varies geographically; East/Central Africa populations ~10 kHz higher than southern. Caves, mines, buildings."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTERIS_THEBAICA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Most widespread African slit-faced bat. Very low intensity broadband FM calls (\u{201c}whispering bat\u{201d}). Multi-harmonic with peaks at 50, 73, 90, 113 kHz. Gleaner that uses passive listening. Very difficult to detect on bat detectors."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_NATALENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Southern and East Africa. Peak ~59 kHz. FM-QCF hockey-stick calls. Forms large cave colonies of thousands. Key cave-roosting species, often sympatric with Rhinolophus."),
        name: None,
    },
    // ── Very Common (non-echolocating) ────────────────────────────
    BookEntryDef {
        species: &species::ROUSETTUS_AEGYPTIACUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Only African fruit bat with true echolocation\u{2014}using tongue clicks (broadband, ~50\u{2013}100 \u{b5}s duration, 12\u{2013}70 kHz). Caves and mines across sub-Saharan Africa and Egypt. Very large colonies."),
        name: None,
    },
    BookEntryDef {
        species: &species::EIDOLON_HELVUM,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Most widespread African megabat. Massive migratory colonies of up to 1 million individuals (Kasanka, Zambia). Critical seed disperser for tropical forests. Uses vision and smell only."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPOMOPHORUS_WAHLBERGI,
        commonness: Some(Commonness::VeryCommon),
        description: Some("East and southern Africa savannas. Males produce loud honking display calls audible at considerable distance. Common in gardens and fruit orchards. No echolocation."),
        name: None,
    },
    // ── Common ───────────────────────────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_CLIVOSUS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in caves across southern and eastern Africa. CF peak varies geographically (~90\u{2013}92 kHz in southern Africa, ~80\u{2013}100 kHz across full range). Often roosts alongside Miniopterus colonies."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_SIMULATOR,
        commonness: Some(Commonness::Common),
        description: Some("Southern and eastern Africa savannas. CF peak ~80 kHz. Often sympatric with R. swinnyi and R. clivosus. Savanna woodland specialist."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_HILDEBRANDTII,
        commonness: Some(Commonness::Common),
        description: Some("Large horseshoe bat of east and southern Africa. CF peak varies 32\u{2013}46 kHz across a species complex (Taylor et al. 2012). Unusually low frequency for a rhinolophid due to large body size."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_FUMIGATUS,
        commonness: Some(Commonness::Common),
        description: Some("Sub-Saharan Africa woodlands and savanna. CF peak shows strong geographic variation: ~53\u{2013}59 kHz (southern), ~62\u{2013}66 kHz (Cameroon), ~45\u{2013}50 kHz (Uganda). Multiple cryptic species likely."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_CAPENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Endemic to South Africa (Western, Eastern, Northern Cape). CF peak ~83\u{2013}86 kHz. Inhabits coastal caves and rocky outcrops. Well-studied acoustically."),
        name: None,
    },
    BookEntryDef {
        species: &species::HIPPOSIDEROS_RUBER,
        commonness: Some(Commonness::Common),
        description: Some("West and Central Africa forests. CF peak ~127\u{2013}131 kHz. Cryptic species pair with H. caffer\u{2014}distinguishable by lower call frequency. Often sympatric."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTOPHILUS_VIRIDIS,
        commonness: Some(Commonness::Common),
        description: Some("East and southern African savannas. Peak ~40\u{2013}47 kHz (geographically variable). Smaller than S. dinganii. Roosts in buildings and tree hollows."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTOPHILUS_LEUCOGASTER,
        commonness: Some(Commonness::Common),
        description: Some("Sahel and West Africa savannas. Peak ~32\u{2013}35 kHz. Largest Scotophilus in Africa. Roosts in buildings and palm trees."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_HESPERIDUS,
        commonness: Some(Commonness::Common),
        description: Some("Sub-Saharan Africa. Peak ~45\u{2013}48 kHz. Small bat frequently detected around human habitation and streetlights. Similar call to P. kuhlii."),
        name: None,
    },
    BookEntryDef {
        species: &species::AFRONYCTERIS_NANUS,
        commonness: Some(Commonness::Common),
        description: Some("Sub-Saharan Africa. Peak ~43 kHz. FM calls lasting 4\u{2013}5 ms. Named for roosting in furled banana leaves. One of Africa's smallest bats (3\u{2013}5 g)."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_RUEPPELLII,
        commonness: Some(Commonness::Common),
        description: Some("North Africa, Sahel, East Africa, and into the Middle East. Peak ~40\u{2013}44 kHz. Associated with arid habitats and waterways."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_TRICOLOR,
        commonness: Some(Commonness::Common),
        description: Some("Southern and East Africa. Broadband FM sweep peaking ~48 kHz. Cave- and mine-roosting. Distinctive tricolored fur pattern."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_KUHLII,
        commonness: Some(Commonness::Common),
        description: Some("North Africa and Mediterranean margin. Peak ~38\u{2013}42 kHz. Common around streetlights in urban areas. Range expanding."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOPS_CONDYLURUS,
        commonness: Some(Commonness::Common),
        description: Some("Sub-Saharan Africa savannas. FM-QCF calls peaking ~25\u{2013}28 kHz. Roosts in buildings and tree hollows. Often in mixed colonies with Chaerephon pumilus."),
        name: None,
    },
    BookEntryDef {
        species: &species::SAUROMYS_PETROPHILUS,
        commonness: Some(Commonness::Common),
        description: Some("Southern Africa. Narrowband QCF calls peaking ~30 kHz. Roosts in rock crevices. Distinctively flattened skull for squeezing into narrow cracks."),
        name: None,
    },
    BookEntryDef {
        species: &species::TAPHOZOUS_MAURITIANUS,
        commonness: Some(Commonness::Common),
        description: Some("Sub-Saharan Africa and Madagascar. Multiharmonic CF search calls with FM approach calls. Fundamental at 11\u{2013}13 kHz (often audible to humans). Roosts on exposed walls and tree trunks."),
        name: None,
    },
    BookEntryDef {
        species: &species::TAPHOZOUS_PERFORATUS,
        commonness: Some(Commonness::Common),
        description: Some("North and East Africa, arid regions. QCF calls peaking ~25\u{2013}28 kHz. Roosts in tombs, ancient ruins, and rock faces."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_FRATERCULUS,
        commonness: Some(Commonness::Common),
        description: Some("Southern Africa. Peak ~71 kHz (12 kHz higher than M. natalensis). Smaller species. Cave-roosting. Distinguished from M. natalensis by higher call frequency."),
        name: None,
    },
    // ── Common (non-echolocating) ────────────────────────────────
    BookEntryDef {
        species: &species::HYPSIGNATHUS_MONSTROSUS,
        commonness: Some(Commonness::Common),
        description: Some("Central and West Africa forests. Largest African bat (wingspan to 90 cm). Males have enlarged larynx and rostrum for loud lek-display calls. No echolocation."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPOMOPS_FRANQUETI,
        commonness: Some(Commonness::Common),
        description: Some("Central and West Africa tropical forests. Males produce repetitive metallic calls during display. Important pollinator and seed disperser. No echolocation."),
        name: None,
    },
    // ── Uncommon ─────────────────────────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_SWINNYI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Eastern South Africa, Eswatini, Zimbabwe. High CF peak ~107 kHz. Clutter forager in dense vegetation. Less common than sympatric R. simulator."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_BLASII,
        commonness: Some(Commonness::Uncommon),
        description: Some("North Africa and East Africa. CF peak ~86 kHz. Mediterranean-type habitats and caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_DARLINGI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Southern Africa woodlands (Zimbabwe, Mozambique, South Africa). Small species with CF peak ~86\u{2013}88 kHz. Rocky habitats and caves."),
        name: None,
    },
    BookEntryDef {
        species: &species::HIPPOSIDEROS_VITTATUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("East and southern Africa. Large species with unusually low CF ~60 kHz for a hipposiderid. Caves and large rock overhangs."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_WELWITSCHII,
        commonness: Some(Commonness::Uncommon),
        description: Some("Sub-Saharan Africa woodlands. Lower peak ~34 kHz than M. tricolor. Short broadband FM calls. Unusual among African Myotis for roosting in foliage."),
        name: None,
    },
    BookEntryDef {
        species: &species::EPTESICUS_HOTTENTOTUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Southern Africa rocky areas. Peak ~30\u{2013}35 kHz. FM-QCF calls. Distinctive long free tail. Roosts in rock crevices."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLAUCONYCTERIS_VARIEGATA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Sub-Saharan Africa savanna and woodland. Broadband FM sweeps 70\u{2013}30 kHz. Distinctive reticulated wing markings. Slow, fluttery flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTICEINOPS_SCHLIEFFENI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Sub-Saharan Africa savannas. Peak ~42 kHz. FM-QCF calls. Often one of the first bats to emerge at dusk. Roosts in buildings and under tree bark."),
        name: None,
    },
    BookEntryDef {
        species: &species::MOPS_MIDAS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Sub-Saharan Africa. Very low frequency QCF peaking ~13\u{2013}16 kHz. Large molossid (40\u{2013}60 g). Calls partially audible to humans. High, fast flight above the canopy."),
        name: None,
    },
    BookEntryDef {
        species: &species::OTOMOPS_MARTIENSSENI,
        commonness: Some(Commonness::Uncommon),
        description: Some("East and southern Africa. Very low frequency calls (~11\u{2013}14 kHz), audible to humans. Very large molossid. Caves and buildings. Individual call signatures documented."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHAEREPHON_ANSORGEI,
        commonness: Some(Commonness::Uncommon),
        description: Some("West, Central, and East Africa forests. QCF calls peaking ~28\u{2013}32 kHz. Forest edge and canopy forager."),
        name: None,
    },
    BookEntryDef {
        species: &species::TADARIDA_FULMINANS,
        commonness: Some(Commonness::Uncommon),
        description: Some("East Africa and Madagascar. Low frequency QCF peaking ~16\u{2013}18 kHz. Large species with long narrow wings for fast open-air flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHAEREPHON_CHAPINI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Central and East Africa. QCF calls peaking ~25\u{2013}28 kHz. Forest and forest edge habitat. Similar call to C. pumilus but more forest-associated."),
        name: None,
    },
    BookEntryDef {
        species: &species::COLEURA_AFRA,
        commonness: Some(Commonness::Uncommon),
        description: Some("East Africa coast. Low-duty-cycle QCF calls peaking at ~33 kHz. Near Threatened. Colonial in coastal caves and rock shelters."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTERIS_GRANDIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Central and West Africa forests. Largest nycterid. Very low intensity broadband FM. Remarkable predator\u{2014}hunts fish, frogs, and smaller bats."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTERIS_MACROTIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("West and Central Africa forests. Low-intensity broadband FM similar to N. thebaica but larger. Gleaner in forest understory."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_INFLATUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Central and East Africa. Larger species with lower peak ~52\u{2013}55 kHz than M. natalensis. Caves in forested areas."),
        name: None,
    },
    // ── Rare ─────────────────────────────────────────────────────
    BookEntryDef {
        species: &species::HIPPOSIDEROS_GIGAS,
        commonness: Some(Commonness::Rare),
        description: Some("West and Central Africa forests. Largest hipposiderid; CF ~60\u{2013}63 kHz. Caves near forest. Rarely encountered."),
        name: None,
    },
    BookEntryDef {
        species: &species::CLOEOTIS_PERCIVALI,
        commonness: Some(Commonness::Rare),
        description: Some("Holds the record for the highest known bat echolocation frequency (~212 kHz). Southern and East Africa caves and mines. Tiny bat (3\u{2013}5 g). Requires very high sample rate detectors (>400 kHz) to record."),
        name: None,
    },
    BookEntryDef {
        species: &species::LAEPHOTIS_BOTSWANAE,
        commonness: Some(Commonness::Rare),
        description: Some("Central and southern Africa. Peak ~37 kHz. Broadband FM. Large ears suggest some gleaning behavior. One of Africa's least-known bat species."),
        name: None,
    },
    BookEntryDef {
        species: &species::KERIVOULA_ARGENTATA,
        commonness: Some(Commonness::Rare),
        description: Some("East and southern Africa. Very high frequency, low-intensity FM calls (~90\u{2013}118 kHz). Clutter specialist in dense vegetation. Very difficult to detect on standard bat detectors."),
        name: None,
    },
    BookEntryDef {
        species: &species::KERIVOULA_LANOSA,
        commonness: Some(Commonness::Rare),
        description: Some("Sub-Saharan Africa forests. Very high frequency broadband FM (~95\u{2013}105 kHz peak). Forest interior specialist. Rarely captured or recorded."),
        name: None,
    },
];

// ══════════════════════════════════════════════════════════════════════════════
// Southeast Asia — species-level entries
// ══════════════════════════════════════════════════════════════════════════════

const SOUTHEAST_ASIA_BOOK: &[BookEntryDef] = &[
    // ── Rhinolophidae ──
    BookEntryDef { species: &species::RHINOLOPHUS_AFFINIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_LUCTUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_PUSILLUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_MALAYANUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_BORNEENSIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_TRIFOLIATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_ACUMINATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_PEARSONII, commonness: None, description: None, name: None },
    // ── Hipposideridae ──
    BookEntryDef { species: &species::HIPPOSIDEROS_ARMIGER, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_LARVATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_BICOLOR, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_GALERITUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_DIADEMA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_POMONA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::COELOPS_FRITHII, commonness: None, description: None, name: None },
    // ── Megadermatidae ──
    BookEntryDef { species: &species::MEGADERMA_LYRA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MEGADERMA_SPASMA, commonness: None, description: None, name: None },
    // ── Emballonuridae ──
    BookEntryDef { species: &species::TAPHOZOUS_LONGIMANUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::TAPHOZOUS_THEOBALDI, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::EMBALLONURA_MONTICOLA, commonness: None, description: None, name: None },
    // ── Molossidae ──
    BookEntryDef { species: &species::CHAEREPHON_PLICATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::OTOMOPS_FORMOSUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MOPS_MOPS, commonness: None, description: None, name: None },
    // ── Vespertilionidae ──
    BookEntryDef { species: &species::MYOTIS_HORSFIELDII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_MURICOLA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_HASSELTII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::KERIVOULA_HARDWICKII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::KERIVOULA_PELLUCIDA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MURINA_CYCLOTIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::TYLONYCTERIS_PACHYPUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::TYLONYCTERIS_ROBUSTULA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::SCOTOPHILUS_KUHLII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_JAVANICUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HESPEROPTENUS_BLANFORDI, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::IA_IO, commonness: None, description: None, name: None },
    // ── Miniopteridae ──
    BookEntryDef { species: &species::MINIOPTERUS_MAGNATER, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MINIOPTERUS_PUSILLUS, commonness: None, description: None, name: None },
    // ── Pteropodidae (non-echolocating except Rousettus) ──
    BookEntryDef { species: &species::ROUSETTUS_AMPLEXICAUDATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::CYNOPTERUS_SPHINX, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::EONYCTERIS_SPELAEA, commonness: None, description: None, name: None },
];

// ══════════════════════════════════════════════════════════════════════════════
// South Asia — species-level entries
// ══════════════════════════════════════════════════════════════════════════════

const SOUTH_ASIA_BOOK: &[BookEntryDef] = &[
    // ── Rhinolophidae ──
    BookEntryDef { species: &species::RHINOLOPHUS_ROUXII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_INDOROUXII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_LEPIDUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_BEDDOMEI, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_AFFINIS, commonness: None, description: Some("NE India, Nepal, Bhutan. CF ~69\u{2013}84 kHz (varies). Often sympatric with R. rouxii."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_LUCTUS, commonness: None, description: Some("Forested hills of northern and NE India, Nepal, Bhutan. CF ~32\u{2013}43 kHz. Cave-roosting."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_FERRUMEQUINUM, commonness: None, description: Some("Northern India, Nepal, Pakistan, Bhutan. CF ~70\u{2013}83 kHz. Caves and mines; hunts large insects over open ground."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_HIPPOSIDEROS, commonness: None, description: Some("Pakistan, northern India, Nepal. CF ~108\u{2013}112 kHz. Eastern edge of range. Caves and buildings near dense vegetation."), name: None },
    // ── Hipposideridae ──
    BookEntryDef { species: &species::HIPPOSIDEROS_SPEORIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_BICOLOR, commonness: None, description: Some("CF ~147\u{2013}161 kHz in South Asia. Peninsular India and Sri Lanka. Caves and rock shelters in forests."), name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_POMONA, commonness: None, description: Some("CF ~145\u{2013}157 kHz. Western Ghats and NE India. Small bat in caves and rock crevices."), name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_LANKADIVA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_DURGADASI, commonness: None, description: None, name: None },
    // ── Megadermatidae ──
    BookEntryDef { species: &species::MEGADERMA_LYRA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MEGADERMA_SPASMA, commonness: None, description: Some("NE India, Bangladesh, Sri Lanka. Low-intensity FM. Moist lowland forests. Caves, hollow trees."), name: None },
    // ── Rhinopomatidae ──
    BookEntryDef { species: &species::RHINOPOMA_HARDWICKII, commonness: None, description: Some("Widespread in Pakistan and NW/central India. QCF ~30\u{2013}35 kHz. Arid and semi-arid zones; ruins and rock crevices."), name: None },
    // ── Emballonuridae ──
    BookEntryDef { species: &species::TAPHOZOUS_MELANOPOGON, commonness: None, description: Some("FM ~28\u{2013}35 kHz. Very widespread; often the most abundant bat at study sites in Sri Lanka. Rock walls, cliffs, cave ceilings."), name: None },
    BookEntryDef { species: &species::TAPHOZOUS_LONGIMANUS, commonness: None, description: Some("Common across peninsular India and Sri Lanka. FM-QCF ~28\u{2013}35 kHz. Exposed surfaces of buildings and rock faces."), name: None },
    BookEntryDef { species: &species::TAPHOZOUS_PERFORATUS, commonness: None, description: Some("Pakistan and NW India arid zones. FM ~25\u{2013}35 kHz. Ruins, rock crevices, old temples."), name: None },
    // ── Molossidae ──
    BookEntryDef { species: &species::CHAEREPHON_PLICATUS, commonness: None, description: Some("QCF ~25\u{2013}30 kHz. Large cave colonies across South Asia. Fast high-altitude forager."), name: None },
    BookEntryDef { species: &species::TADARIDA_AEGYPTIACA, commonness: None, description: Some("Pakistan through India and Sri Lanka. QCF ~18\u{2013}23 kHz. High-flying open-air forager in arid and urban habitats."), name: None },
    BookEntryDef { species: &species::OTOMOPS_WROUGHTONI, commonness: None, description: None, name: None },
    // ── Vespertilionidae ──
    BookEntryDef { species: &species::SCOTOPHILUS_HEATHII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::SCOTOPHILUS_KUHLII, commonness: None, description: Some("FM-QCF ~44\u{2013}50 kHz. Common across South Asia. Smaller than S. heathii. Urban areas."), name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_CEYLONICUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_COROMANDRA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_HORSFIELDII, commonness: None, description: Some("Western Ghats and Sri Lanka. FM ~100\u{2013}35 kHz. Trawls for invertebrates near water."), name: None },
    BookEntryDef { species: &species::MYOTIS_MONTIVAGUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::IA_IO, commonness: None, description: Some("NE India (Meghalaya). FM-QCF ~24\u{2013}30 kHz. Among the world\u{2019}s largest insectivorous bats. Hill forest caves."), name: None },
    BookEntryDef { species: &species::KERIVOULA_PICTA, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MURINA_CYCLOTIS, commonness: None, description: None, name: None },
    // ── Miniopteridae ──
    BookEntryDef { species: &species::MINIOPTERUS_MAGNATER, commonness: None, description: Some("FM ~44\u{2013}55 kHz. Largest Miniopterus in South Asia. Previously misidentified as M. schreibersii. Large cave colonies across India and Sri Lanka."), name: None },
    // ── Pteropodidae ──
    BookEntryDef { species: &species::ROUSETTUS_LESCHENAULTII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::CYNOPTERUS_SPHINX, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PTEROPUS_GIGANTEUS, commonness: None, description: None, name: None },
];

// ══════════════════════════════════════════════════════════════════════════════
// East Asia — species-level entries (China, Korea, Taiwan, Mongolia)
// ══════════════════════════════════════════════════════════════════════════════

const EAST_ASIA_BOOK: &[BookEntryDef] = &[
    // ── Rhinolophidae ──
    BookEntryDef { species: &species::RHINOLOPHUS_FERRUMEQUINUM, commonness: None, description: Some("Very common across China and Korea. CF ~67\u{2013}84 kHz (varies by population). Caves and mines."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_AFFINIS, commonness: None, description: Some("South and central China. CF varies by subspecies: ~87 kHz (Yunnan) to ~74 kHz (eastern China). Reservoir host of SARS-like coronaviruses."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_SINICUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_PUSILLUS, commonness: None, description: Some("Southern China and Taiwan. CF ~103\u{2013}108 kHz. Caves and rock crevices."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_PEARSONII, commonness: None, description: Some("South and central China. CF ~58\u{2013}70 kHz (higher in eastern China). Hilly and montane forest."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_LUCTUS, commonness: None, description: Some("South China (Yunnan, Guangxi, Sichuan). CF ~30\u{2013}35 kHz. Primary forest. Solitary roosts in caves and tree hollows."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_MACROTIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_MONOCEROS, commonness: None, description: None, name: None },
    // ── Hipposideridae ──
    BookEntryDef { species: &species::HIPPOSIDEROS_ARMIGER, commonness: None, description: Some("South and central China and Taiwan. CF ~65\u{2013}75 kHz. Performs Doppler-shift compensation. Large cave colonies."), name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_PRATTI, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::HIPPOSIDEROS_LARVATUS, commonness: None, description: Some("Southern China. CF ~83\u{2013}98 kHz (cryptic species complex, two phonic types). Caves."), name: None },
    BookEntryDef { species: &species::ASELLISCUS_STOLICZKANUS, commonness: None, description: None, name: None },
    // ── Megadermatidae ──
    BookEntryDef { species: &species::MEGADERMA_LYRA, commonness: None, description: Some("South China (Yunnan, Guangxi, Guangdong, Hainan). Carnivorous gleaner; locates prey by passive listening."), name: None },
    // ── Emballonuridae ──
    BookEntryDef { species: &species::TAPHOZOUS_MELANOPOGON, commonness: None, description: Some("SW China (Guangxi, Yunnan, Hainan). FM ~25\u{2013}40 kHz. Rock walls and cave ceilings."), name: None },
    // ── Molossidae ──
    BookEntryDef { species: &species::TADARIDA_INSIGNIS, commonness: None, description: Some("China (Hebei, Beijing, Heilongjiang), Korea, Russia. FM-QCF ~18\u{2013}25 kHz. Fast high-altitude forager."), name: None },
    BookEntryDef { species: &species::CHAEREPHON_PLICATUS, commonness: None, description: Some("Southern China (Yunnan, Guangxi, Guangdong). QCF ~25\u{2013}30 kHz. Enormous cave colonies."), name: None },
    // ── Miniopteridae ──
    BookEntryDef { species: &species::MINIOPTERUS_FULIGINOSUS, commonness: None, description: Some("Widespread in China, Korea, Taiwan. FM ~44\u{2013}50 kHz. Fast agile flier. Cave-roosting."), name: None },
    // ── Vespertilionidae ──
    BookEntryDef { species: &species::MYOTIS_PILOSUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_CHINENSIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_DAVIDII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_FORMOSUS, commonness: None, description: Some("Widespread across China, Taiwan, and Korea. FM ~35\u{2013}95 kHz. Distinctive orange-brown coloring. Forest insectivore."), name: None },
    BookEntryDef { species: &species::MYOTIS_FRATER, commonness: None, description: Some("China and Korea. FM ~110\u{2013}50 kHz. Edge-space forager near cliffs and caves."), name: None },
    BookEntryDef { species: &species::MYOTIS_IKONNIKOVI, commonness: None, description: Some("China, Korea, Russia. FM peak ~50 kHz. Small forest bat."), name: None },
    BookEntryDef { species: &species::MYOTIS_MACRODACTYLUS, commonness: None, description: Some("China and Korea. FM ~90\u{2013}40 kHz. Trawling insectivore over water."), name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_ABRAMUS, commonness: None, description: Some("Extremely common across China, Korea, Taiwan. FM-QCF ~52 kHz peak. Most common urban bat in East Asia."), name: None },
    BookEntryDef { species: &species::HYPSUGO_ALASCHANICUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::NYCTALUS_PLANCYI, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::NYCTALUS_AVIATOR, commonness: None, description: Some("China and Korea. FM-QCF ~21 kHz peak. Forages up to 300 m altitude. Occasionally preys on migrating birds."), name: None },
    BookEntryDef { species: &species::VESPERTILIO_SINENSIS, commonness: None, description: Some("Widespread across China, Korea, Russia. FM-QCF ~24 kHz peak. Migratory. Tree hollows and buildings."), name: None },
    BookEntryDef { species: &species::EPTESICUS_SEROTINUS, commonness: None, description: Some("China and Korea. FM-QCF ~29\u{2013}32 kHz peak. Open habitats and urban areas."), name: None },
    BookEntryDef { species: &species::EPTESICUS_NILSSONII_JP, commonness: None, description: Some("Mongolia, northern China, Korea. FM-QCF ~30\u{2013}32 kHz. The world\u{2019}s most northerly bat."), name: Some("Northern Bat") },
    BookEntryDef { species: &species::VESPERTILIO_MURINUS, commonness: None, description: Some("Mongolia and northern China. FM-QCF alternating calls ~26 kHz peak. Open steppe habitats."), name: None },
    BookEntryDef { species: &species::BARBASTELLA_LEUCOMELAS, commonness: None, description: Some("China, Korea, Mongolia. Alternating FM: type A ~32\u{2013}37 kHz, type B ~38\u{2013}45 kHz. Forest-dwelling."), name: None },
    BookEntryDef { species: &species::MURINA_LEUCOGASTER, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MURINA_HILGENDORFI, commonness: None, description: Some("China and Korea. Ultra-broadband FM ~165\u{2192}45 kHz. Forest gleaner."), name: None },
    BookEntryDef { species: &species::IA_IO, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::SCOTOMANES_ORNATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::SCOTOPHILUS_KUHLII, commonness: None, description: Some("Southern China and Taiwan. QCF ~44\u{2013}47 kHz. Open-country forager; buildings and palms."), name: None },
    // ── Pteropodidae ──
    BookEntryDef { species: &species::ROUSETTUS_LESCHENAULTII, commonness: None, description: Some("South and central China, Taiwan. Tongue-click echolocation. Cave-roosting fruit bat."), name: None },
    BookEntryDef { species: &species::CYNOPTERUS_SPHINX, commonness: None, description: Some("Southern China and Taiwan. Tent-roosting fruit bat. Important pollinator."), name: None },
];

// ══════════════════════════════════════════════════════════════════════════════
// Middle East — species-level entries (incl. Central Asia)
// ══════════════════════════════════════════════════════════════════════════════

const MIDDLE_EAST_BOOK: &[BookEntryDef] = &[
    // ── Rhinolophidae ──
    BookEntryDef { species: &species::RHINOLOPHUS_FERRUMEQUINUM, commonness: None, description: Some("Very common across Turkey, Levant, Iran, Central Asia. CF ~78\u{2013}84 kHz. Caves and mines."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_HIPPOSIDEROS, commonness: None, description: Some("Turkey through Iran. CF ~105\u{2013}115 kHz. Caves and buildings near dense vegetation."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_EURYALE, commonness: None, description: Some("Turkey, Levant, Iran. CF ~100\u{2013}108 kHz. Cave-roosting."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_BLASII, commonness: None, description: Some("Middle East caves. CF ~86 kHz. Often sympatric with R. euryale and R. mehelyi."), name: None },
    BookEntryDef { species: &species::RHINOLOPHUS_MEHELYI, commonness: None, description: None, name: None },
    // ── Hipposideridae ──
    BookEntryDef { species: &species::ASELLIA_TRIDENS, commonness: None, description: None, name: None },
    // ── Rhinopomatidae ──
    BookEntryDef { species: &species::RHINOPOMA_MICROPHYLLUM, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOPOMA_HARDWICKII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::RHINOPOMA_CYSTOPS, commonness: None, description: None, name: None },
    // ── Emballonuridae ──
    BookEntryDef { species: &species::TAPHOZOUS_NUDIVENTRIS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::TAPHOZOUS_PERFORATUS, commonness: None, description: Some("Egypt, Levant, Arabia. QCF ~25\u{2013}28 kHz. Arid regions; tombs, ruins, and rock faces."), name: None },
    // ── Molossidae ──
    BookEntryDef { species: &species::TADARIDA_TENIOTIS, commonness: None, description: Some("Turkey, Israel, Arabia. QCF ~10\u{2013}14 kHz, partially audible to humans. Cliff and building crevices."), name: None },
    BookEntryDef { species: &species::TADARIDA_AEGYPTIACA, commonness: None, description: Some("Widespread across entire region. QCF ~18\u{2013}23 kHz. High-flying open-air forager."), name: None },
    // ── Miniopteridae ──
    BookEntryDef { species: &species::MINIOPTERUS_SCHREIBERSII, commonness: None, description: Some("Turkey, Levant, Iran. FM ~47\u{2013}57 kHz. Large cave colonies."), name: None },
    BookEntryDef { species: &species::MINIOPTERUS_PALLIDUS, commonness: None, description: None, name: None },
    // ── Vespertilionidae ──
    BookEntryDef { species: &species::OTONYCTERIS_HEMPRICHII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PLECOTUS_CHRISTII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_MYOTIS, commonness: None, description: Some("Turkey, Iran, Levant caves. FM ~25\u{2013}80 kHz. Often in large cave colonies."), name: None },
    BookEntryDef { species: &species::MYOTIS_BLYTHII, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::MYOTIS_EMARGINATUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_KUHLII, commonness: None, description: Some("Dominant urban pipistrelle across the entire region. FM ~35\u{2013}45 kHz peak."), name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_PIPISTRELLUS, commonness: None, description: Some("Turkey, Caucasus, northern Levant. FM ~42\u{2013}51 kHz peak."), name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_DESERTI, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::PIPISTRELLUS_RUEPPELLII, commonness: None, description: Some("Arid zones and waterways across the region. FM-QCF ~40\u{2013}44 kHz peak."), name: None },
    BookEntryDef { species: &species::HYPSUGO_SAVII, commonness: None, description: Some("Mediterranean Turkey, Levant, Iran. FM ~28\u{2013}42 kHz peak."), name: None },
    BookEntryDef { species: &species::EPTESICUS_SEROTINUS, commonness: None, description: Some("Turkey, Iran, Caucasus. FM-QCF ~22\u{2013}55 kHz. Open habitats."), name: None },
    BookEntryDef { species: &species::EPTESICUS_BOTTAE, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::EPTESICUS_ISABELLINUS, commonness: None, description: None, name: None },
    BookEntryDef { species: &species::NYCTALUS_NOCTULA, commonness: None, description: Some("Turkey, Iran, Central Asia. QCF ~18\u{2013}25 kHz. Fast high-altitude forager."), name: None },
    BookEntryDef { species: &species::VESPERTILIO_MURINUS, commonness: None, description: Some("Central Asia (Kazakhstan, Uzbekistan, Kyrgyzstan). FM-QCF alternating ~26 kHz. Open steppe."), name: None },
    BookEntryDef { species: &species::BARBASTELLA_LEUCOMELAS, commonness: None, description: Some("Turkey, Iran, Afghanistan, Central Asia. Alternating FM types. Forest-dwelling."), name: None },
    // ── Nycteridae ──
    BookEntryDef { species: &species::NYCTERIS_THEBAICA, commonness: None, description: Some("Sinai and Arabian margins. Very low intensity FM. Gleaner; almost undetectable on bat detectors."), name: None },
    // ── Pteropodidae ──
    BookEntryDef { species: &species::ROUSETTUS_AEGYPTIACUS, commonness: None, description: Some("Large cave colonies in Israel, Turkey, Egypt. Tongue-click echolocation. Key Middle Eastern cave bat."), name: None },
];

// ══════════════════════════════════════════════════════════════════════════════
// Australia — species-level entries sorted by commonness within families
// ══════════════════════════════════════════════════════════════════════════════
//
// Sources:
// - Pennay, Law & Reinhold (2004): Bat Calls of New South Wales
// - Reinhold et al. (2001): Key to the bat calls of SE Queensland & NE NSW
// - Churchill (2008): Australian Bats (2nd ed.)
// - Milne (2002): The Pilbara microbat calls (WA Museum Records)
// - Armstrong & Reardon (2006): Rhinonicteris aurantia call frequency variation
// - DCCEEW (Dept. Climate Change, Energy, Environment and Water) species profiles

const AUSTRALIA_BOOK: &[BookEntryDef] = &[
    // ── Rhinolophidae — Horseshoe bats ────────────────────────────
    BookEntryDef {
        species: &species::RHINOLOPHUS_MEGAPHYLLUS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread in eastern Australia from Queensland to Victoria. Constant-frequency CF calls at ~68\u{2013}70 kHz. Cave-roosting in forests and woodlands. Uses horseshoe-shaped noseleaf to direct calls."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINOLOPHUS_ROBERTSI,
        commonness: Some(Commonness::Rare),
        description: Some("Restricted to tropical north Queensland. Lowest echolocation frequency of any rhinolophid (~28\u{2013}34 kHz CF). Vulnerable (EPBC Act). Cave-roosting in warm humid caves and old mines."),
        name: None,
    },
    // ── Hipposideridae — Leaf-nosed bats ──────────────────────────
    BookEntryDef {
        species: &species::HIPPOSIDEROS_ATER,
        commonness: Some(Commonness::Uncommon),
        description: Some("Very high-frequency CF echolocation (~130\u{2013}138 kHz). Small hipposiderid of tropical woodlands and monsoon forests in northern Australia. Often beyond range of standard bat detectors."),
        name: None,
    },
    BookEntryDef {
        species: &species::RHINONICTERIS_AURANTIA,
        commonness: Some(Commonness::Uncommon),
        description: Some("Australia's only Rhinonicteris. High-frequency CF calls (~114\u{2013}121 kHz) with geographic variation between Pilbara and Top End populations. Obligate cave-dweller. Vulnerable (EPBC Act)."),
        name: None,
    },
    BookEntryDef {
        species: &species::HIPPOSIDEROS_DIADEMA_AU,
        commonness: Some(Commonness::Rare),
        description: Some("Largest Australian hipposiderid. CF ~58\u{2013}63 kHz. Cave-roosting in tropical rainforest of far north Queensland. Striking pale shoulder markings. Long-distance forager. Near Threatened."),
        name: Some("Diadem Leaf-nosed Bat"),
    },
    // ── Megadermatidae — Ghost Bat ────────────────────────────────
    BookEntryDef {
        species: &species::MACRODERMA_GIGAS,
        commonness: Some(Commonness::Rare),
        description: Some("Australia's only megadermatid and largest carnivorous bat. Very quiet broadband FM calls (20\u{2013}56 kHz). Hunts vertebrate prey including other bats, lizards, and frogs. Vulnerable (EPBC Act)."),
        name: None,
    },
    // ── Emballonuridae — Sheathtail bats ──────────────────────────
    BookEntryDef {
        species: &species::SACCOLAIMUS_FLAVIVENTRIS,
        commonness: Some(Commonness::Common),
        description: Some("Large, fast-flying bat with glossy black fur and yellow belly. QCF ~18\u{2013}22 kHz. Migratory; high-altitude open-air forager across northern and eastern Australia."),
        name: None,
    },
    BookEntryDef {
        species: &species::TAPHOZOUS_GEORGIANUS,
        commonness: Some(Commonness::Common),
        description: Some("Widespread across northern and western Australia. QCF search calls peaking ~25 kHz. Roosts in caves, rock crevices, and abandoned mines. Fast, direct flight in open habitats."),
        name: None,
    },
    BookEntryDef {
        species: &species::SACCOLAIMUS_SACCOLAIMUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Critically Endangered (EPBC Act). Narrow-band QCF calls averaging ~20 kHz. Large sheathtail bat of tropical woodlands in far north Queensland and Top End. Extremely rarely recorded."),
        name: Some("Bare-rumped Sheathtail Bat"),
    },
    BookEntryDef {
        species: &species::TAPHOZOUS_AUSTRALIS,
        commonness: Some(Commonness::Rare),
        description: Some("Flat to slightly sloped QCF calls at 23\u{2013}27 kHz. Restricted to coastal caves and boulder piles along Queensland and NT coasts. Rare and poorly known."),
        name: None,
    },
    BookEntryDef {
        species: &species::TAPHOZOUS_TROUGHTONI,
        commonness: Some(Commonness::Rare),
        description: Some("Low-frequency QCF calls below 25 kHz. Roosts in sandstone caves and rocky escarpments in inland Queensland and western NSW. Rarely recorded; poorly known ecology."),
        name: None,
    },
    // ── Molossidae — Free-tailed bats ─────────────────────────────
    BookEntryDef {
        species: &species::AUSTRONOMUS_AUSTRALIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Australia's largest insectivorous bat. Loud, low-frequency QCF calls (~11\u{2013}14 kHz) audible to some humans. Fast, high-flying open-air forager across most of Australia."),
        name: None,
    },
    BookEntryDef {
        species: &species::OZIMOPS_PLANICEPS,
        commonness: Some(Commonness::Common),
        description: Some("Small free-tailed bat of south-eastern Australia. QCF search calls at ~26\u{2013}29 kHz. Roosts in tree hollows and buildings. Rapid, direct flight."),
        name: None,
    },
    BookEntryDef {
        species: &species::OZIMOPS_RIDEI,
        commonness: Some(Commonness::Common),
        description: Some("Widespread across eastern Australian coasts. QCF search calls at ~30\u{2013}35 kHz. Similar to Southern Free-tailed Bat but slightly higher frequency. Tree-hollow roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHAEREPHON_JOBENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Very loud, low-frequency QCF calls (16\u{2013}23 kHz) often audible to humans. Large free-tailed bat of tropical northern Australia. Fast, high-altitude forager over woodland."),
        name: None,
    },
    BookEntryDef {
        species: &species::OZIMOPS_LUMSDENAE,
        commonness: Some(Commonness::Common),
        description: Some("Low-frequency QCF calls peaking ~24 kHz. Largest Ozimops species. Formerly Mormopterus beccarii (in part). Widespread across tropical and subtropical northern Australia."),
        name: Some("Northern Free-tailed Bat (Lumsden's)"),
    },
    BookEntryDef {
        species: &species::MICRONOMUS_NORFOLKENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("QCF search calls at ~32\u{2013}35 kHz. Small free-tailed bat of coastal eastern Australia from SE Queensland to southern NSW. Vulnerable (EPBC Act). Roosts in tree hollows and under bark."),
        name: None,
    },
    // ── Miniopteridae — Bent-winged bats ──────────────────────────
    BookEntryDef {
        species: &species::MINIOPTERUS_ORIANAE_OCEANENSIS,
        commonness: Some(Commonness::Common),
        description: Some("Cave-roosting bat found along eastern Australia. FM calls at ~43\u{2013}48 kHz. Fast, agile flier. Forms large maternity colonies. Vulnerable in some states."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_AUSTRALIS,
        commonness: Some(Commonness::Common),
        description: Some("High-frequency FM calls (57\u{2013}64 kHz). Smaller than the Eastern Bent-winged Bat. Cave-roosting in eastern Australia from Cape York to northern NSW. Often in mixed-species colonies."),
        name: None,
    },
    BookEntryDef {
        species: &species::MINIOPTERUS_ORIANAE_BASSANII,
        commonness: Some(Commonness::Endangered),
        description: Some("Critically Endangered (EPBC Act). FM calls similar to Eastern Bent-winged Bat (~43\u{2013}48 kHz). Dependent on a single maternity cave near Warrnambool. Southwest Victoria only."),
        name: None,
    },
    // ── Vespertilionidae — Vesper bats ────────────────────────────
    BookEntryDef {
        species: &species::CHALINOLOBUS_GOULDII,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Australia's most commonly recorded bat. FM calls with alternating frequencies distinctive (~25\u{2013}34 kHz). Roosts in tree hollows, buildings, and bat boxes across the continent."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHALINOLOBUS_MORIO,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Common across southern Australia. FM calls at ~48\u{2013}53 kHz. Small, dark bat roosting in tree hollows and buildings. Higher frequency calls than Gould's Wattled Bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_GEOFFROYI,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Australia's most widespread bat. Very quiet broadband FM calls (35\u{2013}80 kHz); often difficult to detect acoustically. Gleaning insectivore with distinctive large ears."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_VULTURNUS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("One of Australia's smallest bats (~4 g). FM calls at ~45\u{2013}53 kHz. Common in forests and urban areas across south-eastern Australia. High-frequency calls."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_REGULUS,
        commonness: Some(Commonness::Common),
        description: Some("Small forest bat found across southern Australia. FM calls at ~40\u{2013}55 kHz. Roosts in tree hollows. Call frequency overlaps with Little Forest Bat."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_GOULDI,
        commonness: Some(Commonness::Common),
        description: Some("Common in forests of eastern Australia. Very quiet FM calls (35\u{2013}80 kHz), similar to Lesser Long-eared Bat. Distinguished by larger size and wetter habitat preference."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_DARLINGTONI,
        commonness: Some(Commonness::Common),
        description: Some("Largest Vespadelus species. FM calls at ~38\u{2013}46 kHz. Found in wet and dry forests of south-eastern Australia including Tasmania."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHALINOLOBUS_NIGROGRISEUS,
        commonness: Some(Commonness::Common),
        description: Some("FM calls with geographic variation (26\u{2013}36 kHz). Widespread across northern and eastern Australia. Medium-sized wattled bat of open woodland and forest edges. Tree-hollow roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTOREPENS_BALSTONI,
        commonness: Some(Commonness::Common),
        description: Some("Widespread across inland Australia. FM calls at ~28\u{2013}34 kHz. Found in drier regions. Similar frequency to Gould's Wattled Bat. Tree-hollow roosting."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTOREPENS_ORION,
        commonness: Some(Commonness::Uncommon),
        description: Some("Robust bat of south-eastern coastal forests. Narrow FM frequency range (~34\u{2013}37 kHz) is distinctive. Roosts in tree hollows."),
        name: None,
    },
    BookEntryDef {
        species: &species::MYOTIS_MACROPUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Australia's only fishing bat. Very quiet FM calls (35\u{2013}80 kHz). Trawls water surfaces with large feet. Found near rivers, lakes, and dams across eastern Australia."),
        name: None,
    },
    BookEntryDef {
        species: &species::FALSISTRELLUS_TASMANIENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large vesper bat of south-eastern forests. FM calls at ~34\u{2013}39 kHz. Roosts in tree hollows and buildings. Vulnerable (IUCN)."),
        name: None,
    },
    BookEntryDef {
        species: &species::SCOTEANAX_RUEPPELLII,
        commonness: Some(Commonness::Uncommon),
        description: Some("Large, robust bat of eastern coastal forests. FM calls at ~30\u{2013}38 kHz. Aggressive predator of large insects and small vertebrates. Near Threatened."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_TROUGHTONI,
        commonness: Some(Commonness::Uncommon),
        description: Some("FM/QCF calls at ~49\u{2013}53 kHz. Cave-roosting bat of eastern Australia. Found in sandstone overhangs, caves, and mine tunnels in woodland and dry forest."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_FINLAYSONI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Steep FM/QCF calls at ~53 kHz. Small bat of arid and semi-arid inland Australia. Roosts in caves, mines, rock crevices, and buildings. Tolerates very hot, dry conditions."),
        name: None,
    },
    BookEntryDef {
        species: &species::PIPISTRELLUS_WESTRALIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("FM calls peaking ~45\u{2013}50 kHz. One of Australia's smallest bats (~3 g). Found along northern coasts from WA through NT to Queensland. Mangrove and coastal woodland specialist."),
        name: None,
    },
    BookEntryDef {
        species: &species::VESPADELUS_BAVERSTOCKI,
        commonness: Some(Commonness::Uncommon),
        description: Some("Small bat of inland Australia. FM calls at ~45\u{2013}50 kHz. Restricted to semi-arid regions of central and western Australia."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_ARNHEMENSIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("Very quiet broadband FM gleaning calls (35\u{2013}75 kHz). Tropical woodlands and monsoon forests of Arnhem Land, Kimberley, and Cape York. Poorly known ecology."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_MAJOR,
        commonness: Some(Commonness::Rare),
        description: Some("Formerly N. timoriensis. Restricted to drier woodlands of inland eastern and southern Australia. Very quiet FM gleaning calls (35\u{2013}65 kHz). Vulnerable (EPBC Act)."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_WALKERI,
        commonness: Some(Commonness::Rare),
        description: Some("Very quiet broadband FM calls. Australia's smallest long-eared bat (~4 g). Found in sandstone escarpments and monsoon forests of the Top End and Kimberley. Gleaning insectivore."),
        name: None,
    },
    BookEntryDef {
        species: &species::CHALINOLOBUS_DWYERI,
        commonness: Some(Commonness::Rare),
        description: Some("Broadband FM calls (~35\u{2013}48 kHz). Distinctive black and white fur pattern. Roosts in sandstone cliff overhangs near fertile areas. Eastern Australia. Vulnerable (EPBC Act)."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHONISCUS_PAPUENSIS,
        commonness: Some(Commonness::Rare),
        description: Some("Extremely broadband FM calls (60\u{2013}155 kHz). Specialist predator of orb-weaving spiders. Roosts in abandoned bird nests. Rare along the east coast from Cape York to southern NSW."),
        name: None,
    },
    BookEntryDef {
        species: &species::NYCTOPHILUS_CORBENI,
        commonness: Some(Commonness::Endangered),
        description: Some("Vulnerable (EPBC Act). Very quiet FM gleaning calls (35\u{2013}65 kHz). Restricted to semi-arid woodlands of inland NSW and Queensland. Extremely rare."),
        name: None,
    },
    // ── Non-echolocating (will be sorted to end by get_manifest) ──
    BookEntryDef {
        species: &species::PTEROPUS_POLIOCEPHALUS,
        commonness: Some(Commonness::Common),
        description: Some("Australia's largest bat (wingspan ~1 m). Does not echolocate. Camps in large colonies along waterways of eastern Australia. Vulnerable (EPBC Act). Key pollinator and seed disperser."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTEROPUS_ALECTO,
        commonness: Some(Commonness::Common),
        description: Some("Does not echolocate. Large flying-fox of tropical and subtropical northern Australia. Roosts in mangroves, rainforest, and paperbark swamps. Camps can exceed 100,000 individuals."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTEROPUS_SCAPULATUS,
        commonness: Some(Commonness::Common),
        description: Some("Does not echolocate. Nomadic, following eucalypt and paperback flowering. Widespread across northern and eastern Australia. Forms massive seasonal camps."),
        name: None,
    },
    BookEntryDef {
        species: &species::PTEROPUS_CONSPICILLATUS,
        commonness: Some(Commonness::Endangered),
        description: Some("Does not echolocate. Endangered (EPBC Act). Restricted to Wet Tropics and Cape York in far north Queensland. Key rainforest pollinator and seed disperser."),
        name: None,
    },
];

/// Get the bat book manifest for a given region.
///
// ══════════════════════════════════════════════════════════════════════════════
// Antarctica — echolocating marine mammals of the Southern Ocean (easter egg)
// ══════════════════════════════════════════════════════════════════════════════

const ANTARCTICA_BOOK: &[BookEntryDef] = &[
    // ── Toothed whales — echolocating ───────────────────────────────────────

    BookEntryDef {
        species: &species::ORCINUS_ORCA,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Abundant throughout Antarctic waters year-round. Type A (large, open-water, hunts minke whales), Type B (pack-ice specialist, hunts seals using wave-washing), and smaller Type C (found in dense pack ice near Ross Sea, hunts fish). All types echolocate with broadband clicks."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHYSETER_MACROCEPHALUS,
        commonness: Some(Commonness::Common),
        description: Some("Males migrate to rich Antarctic feeding grounds in summer, diving deep for squid. Produces the most powerful biosonar on Earth — clicks exceeding 230 dB. The massive spermaceti organ focuses sound into a directional beam for hunting at abyssal depths. Females and calves stay in warmer waters."),
        name: None,
    },
    BookEntryDef {
        species: &species::LAGENORHYNCHUS_CRUCIGER,
        commonness: Some(Commonness::Common),
        description: Some("The only small dolphin endemic to Antarctic and sub-Antarctic waters. Distinctive black-and-white hourglass pattern. Echolocates with broadband clicks. Among the least-studied dolphins on Earth — almost everything known comes from at-sea sightings and strandings. Often bow-rides with vessels."),
        name: None,
    },
    BookEntryDef {
        species: &species::GLOBICEPHALA_MELAS,
        commonness: Some(Commonness::Common),
        description: Some("Common in sub-Antarctic and Southern Ocean waters. Highly social; travels in large, tight pods. Echolocates with broadband clicks during deep squid-hunting dives. Also produces complex pulsed social calls. Known for mass strandings, particularly in New Zealand and Tasmania."),
        name: None,
    },
    BookEntryDef {
        species: &species::LISSODELPHIS_PERONII,
        commonness: Some(Commonness::Uncommon),
        description: Some("Graceful, finless dolphin of the Southern Ocean — unique among southern dolphins in lacking a dorsal fin. Travels in large schools, sometimes thousands strong. Echolocates with broadband clicks. Circumpolar in cool to sub-Antarctic waters. Swift and acrobatic."),
        name: None,
    },
    BookEntryDef {
        species: &species::HYPEROODON_PLANIFRONS,
        commonness: Some(Commonness::Uncommon),
        description: Some("The most frequently sighted beaked whale in Antarctic waters. Deep-diving; forages with frequency-modulated echolocation clicks. Males develop a prominent, bulging forehead with age. Circumpolar south of 30°S. Curious and approachable; sometimes spy-hops near ships."),
        name: None,
    },
    BookEntryDef {
        species: &species::BERARDIUS_ARNUXII,
        commonness: Some(Commonness::Rare),
        description: Some("Largest beaked whale in the Southern Hemisphere. Echolocates with FM clicks during deep dives. Two teeth at the tip of the lower jaw erupt in both sexes. Occasionally sighted near the Antarctic ice edge. Very poorly known; much of its biology is inferred from its northern relative, Baird's Beaked Whale."),
        name: None,
    },
    BookEntryDef {
        species: &species::MESOPLODON_LAYARDII,
        commonness: Some(Commonness::Rare),
        description: Some("A bizarre beaked whale: males grow long, strap-shaped teeth that curve over the upper jaw, eventually preventing it from opening more than a few centimetres. Despite this, they catch squid by suction feeding. Uses FM echolocation clicks. Circumpolar in sub-Antarctic waters; rarely seen alive."),
        name: None,
    },
    BookEntryDef {
        species: &species::PHOCOENA_DIOPTRICA,
        commonness: Some(Commonness::Rare),
        description: Some("Cryptic sub-Antarctic porpoise with dark eye rings. Uses narrow-band high-frequency (NBHF) clicks at ~130 kHz — acoustic camouflage against orca predation, exploiting the orca's poor high-frequency hearing. Among the least-known cetaceans; almost all records are from strandings on Tierra del Fuego and the Falkland Islands."),
        name: None,
    },

    // ── Baleen whales — non-echolocating (analogous to fruit bats) ──────────

    BookEntryDef {
        species: &species::MEGAPTERA_NOVAEANGLIAE,
        commonness: Some(Commonness::VeryCommon),
        description: Some("No echolocation. Migrates thousands of kilometres from tropical breeding grounds to Antarctic krill-rich feeding waters each summer. Males sing haunting, complex songs lasting up to 30 minutes. Uses bubble-net feeding — spiralling underwater while blowing bubbles to corral krill into dense columns."),
        name: None,
    },
    BookEntryDef {
        species: &species::BALAENOPTERA_BONAERENSIS,
        commonness: Some(Commonness::VeryCommon),
        description: Some("No echolocation. The most abundant baleen whale in Antarctic waters, commonly seen in pack ice. Source of the mysterious 'bio-duck' signal — a repetitive quacking sound recorded for decades before being attributed to this species via acoustic tags. Small for a rorqual but plays a huge role in the krill ecosystem."),
        name: None,
    },
    BookEntryDef {
        species: &species::BALAENOPTERA_MUSCULUS,
        commonness: Some(Commonness::Rare),
        description: Some("No echolocation. The largest animal that has ever lived — up to 30 m and 180 tonnes. Produces infrasonic calls (10–40 Hz) audible across entire ocean basins. Antarctic blue whales (B. m. intermedia) were hunted to near-extinction; fewer than 3,000 may remain. A single blue whale can consume 4 tonnes of krill per day."),
        name: None,
    },
    BookEntryDef {
        species: &species::BALAENOPTERA_PHYSALUS,
        commonness: Some(Commonness::Uncommon),
        description: Some("No echolocation. Second-largest animal on Earth. Produces powerful 20 Hz pulses — among the loudest sustained biological sounds. Distinctive asymmetric colouration: white right lower jaw, dark left. Nicknamed the 'greyhound of the sea' for its speed. Summers in Antarctic waters feeding on krill and small fish."),
        name: None,
    },
    BookEntryDef {
        species: &species::EUBALAENA_AUSTRALIS,
        commonness: Some(Commonness::Uncommon),
        description: Some("No echolocation. Slow, rotund whale with callosities — rough white skin patches unique to each individual, used for photo-ID. Produces low-frequency moans and up-calls. Named the 'right' whale to hunt because it floated when dead. Ranges into sub-Antarctic waters to feed. Recovering from near-extinction."),
        name: None,
    },

    // ── Seals — non-echolocating ────────────────────────────────────────────

    BookEntryDef {
        species: &species::LEPTONYCHOTES_WEDDELLII,
        commonness: Some(Commonness::VeryCommon),
        description: Some("Not echolocation. The southernmost breeding mammal, living on Antarctic fast ice year-round. Produces an extraordinary repertoire of underwater sounds: eerie descending trills, chirps, and sci-fi whistles that can be heard through the ice. Maintains breathing holes by grinding ice with its teeth. Dives to 600+ m."),
        name: None,
    },
    BookEntryDef {
        species: &species::HYDRURGA_LEPTONYX,
        commonness: Some(Commonness::Common),
        description: Some("Not echolocation. Solitary apex predator of Antarctic pack ice. Males produce haunting, pulsing underwater trills and low broadcast calls during breeding season. Feeds on penguins (ambushing them at ice edges), krill, fish, and even other seals. Sinuous and powerful, with a massive gape and lobed teeth for krill filtering."),
        name: None,
    },
];

/// Non-echolocating species are always sorted to the end (stable sort preserves
/// relative order within each group).
pub fn get_manifest(region: BatBookRegion) -> BatBookManifest {
    let book: &[BookEntryDef] = match region {
        BatBookRegion::Australia => AUSTRALIA_BOOK,
        BatBookRegion::VicAustralia => VIC_AUSTRALIA_BOOK,
        BatBookRegion::Europe => EUROPE_BOOK,
        BatBookRegion::CostaRica => COSTA_RICA_BOOK,
        BatBookRegion::Japan => JAPAN_BOOK,
        BatBookRegion::UK => UK_BOOK,
        BatBookRegion::NorthAmerica => NORTH_AMERICA_BOOK,
        BatBookRegion::SouthAmerica => SOUTH_AMERICA_BOOK,
        BatBookRegion::Africa => AFRICA_BOOK,
        BatBookRegion::SoutheastAsia => SOUTHEAST_ASIA_BOOK,
        BatBookRegion::SouthAsia => SOUTH_ASIA_BOOK,
        BatBookRegion::EastAsia => EAST_ASIA_BOOK,
        BatBookRegion::MiddleEast => MIDDLE_EAST_BOOK,
        BatBookRegion::Antarctica => ANTARCTICA_BOOK,
        _ => GLOBAL_BOOK,
    };
    let mut entries: Vec<_> = book.iter().map(|e| e.materialize()).collect();
    // Stable sort: echolocating first, non-echolocating last
    entries.sort_by_key(|e| if e.echolocates { 0u8 } else { 1 });
    BatBookManifest {
        region: region.short_label().to_string(),
        entries,
    }
}
