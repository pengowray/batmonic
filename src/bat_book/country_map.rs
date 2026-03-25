use super::types::BatBookRegion;

/// Map a country name (as used by Xeno-Canto `cnt` field) to the best bat book region.
/// Returns `None` for unrecognised countries.
pub fn country_to_region(country: &str) -> Option<BatBookRegion> {
    // Normalise: trim, lowercase
    let key: String = country.trim().to_lowercase();
    match key.as_str() {
        // ── Countries with their own dedicated book ──────────────────
        "united kingdom" | "uk" | "england" | "scotland" | "wales" | "northern ireland" => {
            Some(BatBookRegion::UK)
        }
        "japan" => Some(BatBookRegion::Japan),
        "costa rica" => Some(BatBookRegion::CostaRica),
        "australia" => Some(BatBookRegion::Australia),

        // ── Europe ───────────────────────────────────────────────────
        "albania" | "andorra" | "austria" | "belarus" | "belgium" | "bosnia and herzegovina"
        | "bulgaria" | "croatia" | "cyprus" | "czech republic" | "czechia" | "denmark"
        | "estonia" | "finland" | "france" | "germany" | "greece" | "hungary" | "iceland"
        | "ireland" | "italy" | "kosovo" | "latvia" | "liechtenstein" | "lithuania"
        | "luxembourg" | "malta" | "moldova" | "monaco" | "montenegro" | "netherlands"
        | "north macedonia" | "norway" | "poland" | "portugal" | "romania"
        | "russian federation" | "russia" | "san marino" | "serbia" | "slovakia" | "slovenia"
        | "spain" | "sweden" | "switzerland" | "ukraine" | "vatican city"
        | "canary islands" | "azores" | "madeira" | "gibraltar" => Some(BatBookRegion::Europe),

        // ── North America ────────────────────────────────────────────
        "united states" | "usa" | "canada" | "mexico" | "bermuda" => {
            Some(BatBookRegion::NorthAmerica)
        }

        // ── Central America & Caribbean → South America (neotropical) ─
        "belize" | "el salvador" | "guatemala" | "honduras" | "nicaragua" | "panama"
        | "cuba" | "jamaica" | "haiti" | "dominican republic" | "puerto rico"
        | "trinidad and tobago" | "barbados" | "saint lucia" | "grenada"
        | "saint vincent and the grenadines" | "antigua and barbuda" | "dominica"
        | "saint kitts and nevis" | "bahamas" | "guadeloupe" | "martinique"
        | "aruba" | "curacao" | "bonaire" => Some(BatBookRegion::SouthAmerica),

        // ── South America ────────────────────────────────────────────
        "argentina" | "bolivia" | "brazil" | "chile" | "colombia" | "ecuador"
        | "french guiana" | "guyana" | "paraguay" | "peru" | "suriname" | "uruguay"
        | "venezuela" | "falkland islands" => Some(BatBookRegion::SouthAmerica),

        // ── Africa ───────────────────────────────────────────────────
        "algeria" | "angola" | "benin" | "botswana" | "burkina faso" | "burundi"
        | "cameroon" | "cape verde" | "cabo verde" | "central african republic" | "chad"
        | "comoros" | "congo" | "democratic republic of the congo"
        | "republic of the congo" | "djibouti" | "equatorial guinea" | "eritrea"
        | "eswatini" | "swaziland" | "ethiopia" | "gabon" | "gambia" | "ghana" | "guinea"
        | "guinea-bissau" | "ivory coast" | "cote d'ivoire" | "kenya" | "lesotho"
        | "liberia" | "libya" | "madagascar" | "malawi" | "mali" | "mauritania"
        | "mauritius" | "morocco" | "mozambique" | "namibia" | "niger" | "nigeria"
        | "rwanda" | "sao tome and principe" | "senegal" | "seychelles" | "sierra leone"
        | "somalia" | "south africa" | "south sudan" | "sudan" | "tanzania" | "togo"
        | "tunisia" | "uganda" | "zambia" | "zimbabwe"
        | "reunion" | "mayotte" | "zanzibar" => Some(BatBookRegion::Africa),

        // ── Middle East ──────────────────────────────────────────────
        "bahrain" | "egypt" | "iran" | "iraq" | "israel" | "jordan" | "kuwait" | "lebanon"
        | "oman" | "palestine" | "qatar" | "saudi arabia" | "syria" | "turkey" | "turkiye"
        | "united arab emirates" | "uae" | "yemen" | "afghanistan" => {
            Some(BatBookRegion::MiddleEast)
        }

        // ── South Asia ───────────────────────────────────────────────
        "bangladesh" | "bhutan" | "india" | "maldives" | "nepal" | "pakistan" | "sri lanka" => {
            Some(BatBookRegion::SouthAsia)
        }

        // ── Southeast Asia ───────────────────────────────────────────
        "brunei" | "cambodia" | "indonesia" | "laos" | "malaysia" | "myanmar" | "burma"
        | "philippines" | "singapore" | "thailand" | "timor-leste" | "east timor"
        | "vietnam" => Some(BatBookRegion::SoutheastAsia),

        // ── East Asia ────────────────────────────────────────────────
        "china" | "hong kong" | "macau" | "mongolia" | "north korea" | "south korea"
        | "taiwan" => Some(BatBookRegion::EastAsia),

        // ── Oceania → closest regional book ──────────────────────────
        "new zealand" | "papua new guinea" | "fiji" | "solomon islands" | "vanuatu"
        | "new caledonia" | "samoa" | "tonga" | "micronesia" | "palau" | "guam"
        | "northern mariana islands" | "marshall islands" | "kiribati" | "tuvalu"
        | "nauru" | "cook islands" | "niue" | "american samoa" | "french polynesia"
        | "wallis and futuna" => Some(BatBookRegion::Australia),

        // ── Central Asia → Middle East (closest) ─────────────────────
        "kazakhstan" | "kyrgyzstan" | "tajikistan" | "turkmenistan" | "uzbekistan" => {
            Some(BatBookRegion::MiddleEast)
        }

        // ── Antarctica ───────────────────────────────────────────────
        "antarctica" | "south georgia" | "south sandwich islands" => {
            Some(BatBookRegion::Antarctica)
        }

        _ => None,
    }
}
