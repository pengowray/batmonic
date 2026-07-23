use super::types::BatBookRegion;

/// A country→region match plus whether the routing is APPROXIMATE — i.e. the
/// country has no dedicated or same-continent book and was sent to the nearest
/// regional book instead. Exact matches are dedicated-country books and
/// same-continent routings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionMatch {
    pub region: BatBookRegion,
    pub approximate: bool,
}

/// Map a country name (as used by Xeno-Canto `cnt` field) to the best bat book
/// region, flagging approximate routings so the UI can mark them as a guess.
/// Returns `None` for unrecognised countries (callers fall back to a favourite
/// or Global).
pub fn country_to_region(country: &str) -> Option<RegionMatch> {
    // Normalise: trim, lowercase
    let key: String = country.trim().to_lowercase();

    // ── Approximate routings: no dedicated/continental book of their own, so
    //    routed to the nearest regional book. Flagged `approximate: true`. ──
    let approx = match key.as_str() {
        // Central America & Caribbean → South America (neotropical)
        "belize"
        | "el salvador"
        | "guatemala"
        | "honduras"
        | "nicaragua"
        | "panama"
        | "cuba"
        | "jamaica"
        | "haiti"
        | "dominican republic"
        | "puerto rico"
        | "trinidad and tobago"
        | "barbados"
        | "saint lucia"
        | "grenada"
        | "saint vincent and the grenadines"
        | "antigua and barbuda"
        | "dominica"
        | "saint kitts and nevis"
        | "bahamas"
        | "guadeloupe"
        | "martinique"
        | "aruba"
        | "curacao"
        | "bonaire" => Some(BatBookRegion::SouthAmerica),

        // Oceania → Australia (closest regional book; note NZ has its own
        // distinct fauna but no dedicated book yet)
        "new zealand"
        | "papua new guinea"
        | "fiji"
        | "solomon islands"
        | "vanuatu"
        | "new caledonia"
        | "samoa"
        | "tonga"
        | "micronesia"
        | "palau"
        | "guam"
        | "northern mariana islands"
        | "marshall islands"
        | "kiribati"
        | "tuvalu"
        | "nauru"
        | "cook islands"
        | "niue"
        | "american samoa"
        | "french polynesia"
        | "wallis and futuna" => Some(BatBookRegion::Australia),

        // Central Asia → Middle East (closest)
        "kazakhstan" | "kyrgyzstan" | "tajikistan" | "turkmenistan" | "uzbekistan" => {
            Some(BatBookRegion::MiddleEast)
        }

        // Bermuda → North America (lone Atlantic island, no own book)
        "bermuda" => Some(BatBookRegion::NorthAmerica),

        _ => None,
    };
    if let Some(region) = approx {
        return Some(RegionMatch {
            region,
            approximate: true,
        });
    }

    // ── Exact routings: dedicated-country books + same-continent. ──
    let region =
        match key.as_str() {
            // Countries with their own dedicated book
            "united kingdom" | "uk" | "england" | "scotland" | "wales" | "northern ireland" => {
                BatBookRegion::UK
            }
            "japan" => BatBookRegion::Japan,
            "costa rica" => BatBookRegion::CostaRica,
            "australia" => BatBookRegion::Australia,
            "greece" => BatBookRegion::Greece,
            "sweden" => BatBookRegion::Sweden,
            "netherlands" | "holland" => BatBookRegion::Netherlands,
            "canada" => BatBookRegion::Canada,
            "united states" | "usa" => BatBookRegion::UnitedStates,
            "mexico" => BatBookRegion::Mexico,
            "kenya" => BatBookRegion::Kenya,
            "eswatini" | "swaziland" => BatBookRegion::Eswatini,

            // Europe
            "albania"
            | "andorra"
            | "austria"
            | "belarus"
            | "belgium"
            | "bosnia and herzegovina"
            | "bulgaria"
            | "croatia"
            | "cyprus"
            | "czech republic"
            | "czechia"
            | "denmark"
            | "estonia"
            | "finland"
            | "france"
            | "germany"
            | "hungary"
            | "iceland"
            | "ireland"
            | "italy"
            | "kosovo"
            | "latvia"
            | "liechtenstein"
            | "lithuania"
            | "luxembourg"
            | "malta"
            | "moldova"
            | "monaco"
            | "montenegro"
            | "north macedonia"
            | "norway"
            | "poland"
            | "portugal"
            | "romania"
            | "russian federation"
            | "russia"
            | "san marino"
            | "serbia"
            | "slovakia"
            | "slovenia"
            | "spain"
            | "switzerland"
            | "ukraine"
            | "vatican city"
            | "canary islands"
            | "azores"
            | "madeira"
            | "gibraltar" => BatBookRegion::Europe,

            // South America (proper)
            "argentina" | "bolivia" | "brazil" | "chile" | "colombia" | "ecuador"
            | "french guiana" | "guyana" | "paraguay" | "peru" | "suriname" | "uruguay"
            | "venezuela" | "falkland islands" => BatBookRegion::SouthAmerica,

            // Africa
            "algeria"
            | "angola"
            | "benin"
            | "botswana"
            | "burkina faso"
            | "burundi"
            | "cameroon"
            | "cape verde"
            | "cabo verde"
            | "central african republic"
            | "chad"
            | "comoros"
            | "congo"
            | "democratic republic of the congo"
            | "republic of the congo"
            | "djibouti"
            | "equatorial guinea"
            | "eritrea"
            | "ethiopia"
            | "gabon"
            | "gambia"
            | "ghana"
            | "guinea"
            | "guinea-bissau"
            | "ivory coast"
            | "cote d'ivoire"
            | "lesotho"
            | "liberia"
            | "libya"
            | "madagascar"
            | "malawi"
            | "mali"
            | "mauritania"
            | "mauritius"
            | "morocco"
            | "mozambique"
            | "namibia"
            | "niger"
            | "nigeria"
            | "rwanda"
            | "sao tome and principe"
            | "senegal"
            | "seychelles"
            | "sierra leone"
            | "somalia"
            | "south africa"
            | "south sudan"
            | "sudan"
            | "tanzania"
            | "togo"
            | "tunisia"
            | "uganda"
            | "zambia"
            | "zimbabwe"
            | "reunion"
            | "mayotte"
            | "zanzibar" => BatBookRegion::Africa,

            // Middle East (proper)
            "bahrain"
            | "egypt"
            | "iran"
            | "iraq"
            | "israel"
            | "jordan"
            | "kuwait"
            | "lebanon"
            | "oman"
            | "palestine"
            | "qatar"
            | "saudi arabia"
            | "syria"
            | "turkey"
            | "turkiye"
            | "united arab emirates"
            | "uae"
            | "yemen"
            | "afghanistan" => BatBookRegion::MiddleEast,

            // South Asia
            "bangladesh" | "bhutan" | "india" | "maldives" | "nepal" | "pakistan" | "sri lanka" => {
                BatBookRegion::SouthAsia
            }

            // Southeast Asia
            "brunei" | "cambodia" | "indonesia" | "laos" | "malaysia" | "myanmar" | "burma"
            | "philippines" | "singapore" | "thailand" | "timor-leste" | "east timor"
            | "vietnam" => BatBookRegion::SoutheastAsia,

            // East Asia
            "china" | "hong kong" | "macau" | "mongolia" | "north korea" | "south korea"
            | "taiwan" => BatBookRegion::EastAsia,

            // Antarctica
            "antarctica" | "south georgia" | "south sandwich islands" => BatBookRegion::Antarctica,

            _ => return None,
        };
    Some(RegionMatch {
        region,
        approximate: false,
    })
}
