//! Identify untouched packages seeded by the previous release without deleting
//! anything from the user's default directory.
use resume_one_themes::sha256;
use std::{fs, path::Path};

pub(super) fn is_previous_default(manifest: &Path) -> bool {
    let Some(directory) = manifest.parent() else {
        return false;
    };
    let Some(name) = directory.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(index) = PACKAGES.binary_search_by_key(&name, |(name, _)| name) else {
        return false;
    };
    let Some(mut files) = fs::read_dir(directory)
        .ok()
        .and_then(|iter| iter.collect::<Result<Vec<_>, _>>().ok())
    else {
        return false;
    };
    if files.len() > 32 {
        return false;
    }
    files.sort_by_key(|entry| entry.file_name());
    let mut fingerprint = String::new();
    for file in files {
        let path = file.path();
        let Ok(name) = file.file_name().into_string() else {
            return false;
        };
        if !fs::symlink_metadata(&path)
            .is_ok_and(|meta| meta.is_file() && meta.len() <= 8 * 1024 * 1024)
        {
            return false;
        }
        let Ok(bytes) = fs::read(path) else {
            return false;
        };
        fingerprint.push_str(&format!("{name}\t{}\n", sha256(&bytes)));
    }
    sha256(fingerprint.as_bytes()) == PACKAGES[index].1
}

// Fingerprints of the packages distributed by v0.1.0-preview.2.
// Source: resume-one-themes@9f5b6c2a5886811119db32f04f4dd16aeb5125e5.
const PACKAGES: &[(&str, &str)] = &[
    (
        "community-academic-cv-lite",
        "3e6d49276e6cdbef97a9826e1b15b0c8f0d29137ba3d07efa5f52802d7cc632d",
    ),
    (
        "community-architects-portfolio",
        "8121f905668954436155a3f46ed814a1b01f00f4b2345595a1626589893a6455",
    ),
    (
        "community-art-deco",
        "68cdb51cd90f01d0f59d67e3319ad87e5b718db0aa7c6c6672e05346b19cb038",
    ),
    (
        "community-art-school-modern",
        "dee29ddda17aff335ea33c0f8e3913fc75285f6c907720d01f71240adb2cdca8",
    ),
    (
        "community-asymmetric-timeline",
        "9f129dfb4b977526670c2e6d8089f6050f20f52cc4a3090416e1ae56cefd098c",
    ),
    (
        "community-berlin-grid",
        "5be685c1d600f278905aa48bb5eea7c1eb28eecc0f81cda32b1c5a03a7e20475",
    ),
    (
        "community-bold-header-statement",
        "7b8fdf39a904089286e9acfaf8e5a02d0af0e56bee946ac770d942482fb5d28c",
    ),
    (
        "community-brutalist",
        "49b0b058f1b0ab32df53f048d217e98b20663ff8dbbdb04a1c23609e3a677cc2",
    ),
    (
        "community-californian-warm",
        "51bd7c5e345adef65a6fd237b706e536f91980aeed2d7b4cc02a011521843cbb",
    ),
    (
        "community-claude",
        "8db86ddae96a51361d085e2fc988e64848eab671676372aaa693daf329e7ef94",
    ),
    (
        "community-clinical-precision",
        "fc6db4bd38e9d471cf5c17f7865c53d7fed6d541e5914b7c23e838e73e5a3ef5",
    ),
    (
        "community-coastal-creative",
        "317fc260af1bd1fe841d872986605192e5a765dc1c601b48754076347643bb50",
    ),
    (
        "community-colophon",
        "dae09ca46ede87cfe81f193c13fcb0de252cb9d8dde07e1c2fd9a0f5af44ed4c",
    ),
    (
        "community-community-garden",
        "5f71fad556bd74a5e6550489f20d8f6c8edf664f7dad9f5e22f81b1d7e43d3db",
    ),
    (
        "community-consultant-polished",
        "041edb334b1709b25fa8dd09af05b89acc0f0c5e0f402070ddc1699c3ca46aa2",
    ),
    (
        "community-creative-confidence",
        "07ff5c9bccfef11a73a1941db18e826cc3bbfb6411b0b9921c0cb05e13dd3a23",
    ),
    (
        "community-creative-studio",
        "06b11534065b6880a54d1672efe80553b6de0af4be4444b2e745a3d6e36210f9",
    ),
    (
        "community-data-driven",
        "98087c0c9ef1a514ad442816afb181f6ec94d112a8f320c74f2c4cd5f6f5023e",
    ),
    (
        "community-desert-modern",
        "26ecea942e02b808c3cca4e7921b75e3db8eaba745f5a6749587bc14fe73699a",
    ),
    (
        "community-developer-mono",
        "19c3e1394af4c2263151df6872b17b5eeef3db2b4ef71a80b436fb5148100cd1",
    ),
    (
        "community-diagonal-accent-bar",
        "cc469e1544c850a1bb80815391d700b8f1f4b5cb5c34760804e506fdd04eafb0",
    ),
    (
        "community-elegant",
        "878ba6e615f2aa5a778331088089a69741c2998bd1b829084f2abeb88ed6c2b8",
    ),
    (
        "community-elegant-pink",
        "c42cff701c4e261c519dc7afb2b0dfb9b0be9a6ba149e08528f64d8033bd7329",
    ),
    (
        "community-even",
        "659ed0cbcda3fe1bdfbdd6aca8d6f65a7fcb67098e74f93ed99682bfb9fdab82",
    ),
    (
        "community-executive-slate",
        "c371790d3928c009bf50ac43ec5425970c69ff1c745ce0a6ec5227cea67c832d",
    ),
    (
        "community-field-researcher",
        "7051ed91044ec15e5ec1ecdfe80f738a8f1b095a7af003d9d16d3440b902ab9d",
    ),
    (
        "community-flat",
        "9a5b63e8955faf9a9b3b6f888756e4c2f6dcfa1bf5f3a02a632ad1ab6b6a0dca",
    ),
    (
        "community-french-atelier",
        "0c69f1c04148b585bb63095b33ffc1647b2493a8050899aa75128ddb9b6f82af",
    ),
    (
        "community-government-standard",
        "b2375eceed8ccd57f07d159ff070b8bee6e3c3586fa5a05581fccf101186f52e",
    ),
    (
        "community-graph-paper-grid",
        "740eb97d42f8309cf08e0022da8bf980ebd2689fc5b5291fe294ff42c3703bca",
    ),
    (
        "community-industrial-engineer",
        "b1d9e8b9f7bf85c448e57757dd0879a122f869f08a4a70960189a736a2213238",
    ),
    (
        "community-investor-brief",
        "27cab49ca82bda651d75e262eeb35c6e27e2b18b09f21f1ed34ddb595cce1574",
    ),
    (
        "community-jacrys",
        "bf57172deb0d6bfc22e84acbd4a10728e2c3c07ac47bc32dbda1c1c2b65248c0",
    ),
    (
        "community-kendall",
        "36a5004d80f5085b4a89351e30cee7e28929927a3166bb55e2e3be6d15d6cbc6",
    ),
    (
        "community-london-bureau",
        "22e1fc6420889d1a5eaa1a1c8dd52cd9397d447232e222c4412c2f45a9473b5f",
    ),
    (
        "community-lucide",
        "17ab11d3a5228c0fa879b793de3665dc2a16d4cfa64dc2e2a84c9547d58a2ec9",
    ),
    (
        "community-macchiato",
        "6e6b8ad030c642989e1af4f2c633aa10480597f36ec7184ca77e0fab72d395d6",
    ),
    (
        "community-marketing-narrative",
        "ce2ca8e1d6261205a0f61e9fb230f11a222d5f8379ac0250ad60e9b682730b8e",
    ),
    (
        "community-mid-century-resume",
        "a116241b500f1f4fefbcb8e92824405cf157faa98054e1276c4e6d617f67187e",
    ),
    (
        "community-minimalist-grid",
        "6e46bdd4685994a094ef7d899a8e8fb2f4fe6364e561e9188aa27ee311035a8f",
    ),
    (
        "community-minyma",
        "86ed67c15831524a65b6f583459f08167e697ac71209733063ab56687a8eb06f",
    ),
    (
        "community-modern-classic",
        "2a8658febc4703e783df6c40bb1dfe463aa554e521bd5e4bb8ee6d590d46a38a",
    ),
    (
        "community-monochrome-noir",
        "adc03d9f4106e7bda91fdc1c64f9b3b148e2409487398603eea70412dd7c77da",
    ),
    (
        "community-new-york-editorial",
        "d5a9e89ba07cda9f428342b7c2ca5a6fc7ce68811d1bc1c7941e2030b6b2b943",
    ),
    (
        "community-nordic-minimal",
        "a76793190dc9bf08632fec450167b06722a927e74f79d21b390fcaf7b8fb5a58",
    ),
    (
        "community-operations-precision",
        "565f07d806b87322cd5d34dea8ef4f11dc7b0f9ac19d0f1abf16285a9d5c141a",
    ),
    (
        "community-pacific-horizon",
        "88dc36ba418e8efeeb8fadd4d1d23b4ffad0f897edf3759e5e3b1ea2320c1b46",
    ),
    (
        "community-paper-plus-plus",
        "4de127124727d5d307d6ceeac1950b2e70dcd008e534ae7182b7a8ee4b39587d",
    ),
    (
        "community-pro",
        "21d14d97cd88305634aa4ec9f485fb3e5241e78d4d1be02f4b035ace430ac26b",
    ),
    (
        "community-product-manager-canvas",
        "19d3c749736b3a6c7784cb60038ab65e1f3660b2e278f92d1ae97ad5504c25e4",
    ),
    (
        "community-professional",
        "f71def29e5591d376c028c7f8a2758b2c8af7b25394a34b466276a65b7a246e7",
    ),
    (
        "community-pumpkin",
        "973303b73c72db93bd167f70be24edeb07fb3edd7215a1f7cfd3f3dc0405729f",
    ),
    (
        "community-reference",
        "d7f69b57636ad661fd5378a66922b4780b452900eecb0b427040100c1e8a28d9",
    ),
    (
        "community-rickosborne",
        "fa7076900b372bb6cfe3ff3389094ee43bdfe1129c557f4dc9af78ff1934e28b",
    ),
    (
        "community-sales-hunter",
        "02c0d4b5622c7e1863deaa69e4531ac13d6da3e68da768cd395d840d056d5f06",
    ),
    (
        "community-sidebar",
        "8c2919e51ef0df1a294aad5a99d96462e4dfa795c3d33efa868a13b3cc1e23a1",
    ),
    (
        "community-sidebar-photo-strip",
        "92438aed9a79d64c60d50eb8c913240f0f91d54c70b313d3f02b0c05d32f4e00",
    ),
    (
        "community-stackoverflow",
        "933521289c4e76e662f22789bd19362f50ffe4f78bfeb5888070f9f3e098d42d",
    ),
    (
        "community-tailwind",
        "d4b9400c30c5d25537f534d0f46b2e62dfa2290ed20a3fef08d0a3f0896a6dfe",
    ),
    (
        "community-tan-responsive",
        "a6d1c1d7515e8bec6f5d7b3a83c4f7c60c9d44339304c278a2145a06ea1d64d6",
    ),
    (
        "community-tokyo-modernist",
        "6517b95cb528d804dd076e602bbf7f44b395a26b547a223b23ce29a3870d5968",
    ),
    (
        "community-two-column",
        "336a213fd5541885532dca555b57a7a5043bd2731059ac633ee4f02ca20b998b",
    ),
    (
        "community-two-column-modernist",
        "1d6ccf67cb3fa332149c3ec916304858e9d650ef8df2cba33e0e4e924606ee61",
    ),
    (
        "community-typewriter-modern",
        "5f38fd53c6c6515f42f7241d96bab8d2b23435fa8a84a8404ee5692fe0cec76b",
    ),
    (
        "community-university-first",
        "3de2cdc9029867cc8df11a954a1c0e9a4ebe09d11e48429eccc7aeb81c1cdd12",
    ),
    (
        "community-urban-techno",
        "fd295e79da2974d5005a63cae255c02dd30c70074470f59fbcaf430b48b260bf",
    ),
    (
        "community-writers-portfolio",
        "99c69ca4937d78f19e05b1244aac563c1c388bf19348c0d1bde6be6793ec6e79",
    ),
];
