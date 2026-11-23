//! The scene labels (`scene_labels.rs`) a question names in words — "nur draußen", "in der Bar", "am Strand",
//! "on the beach", "在海边" — in the app's 16 languages, so that a chat question searches among the pictures with
//! that label as well (services `commands/place_scene_filter.rs::question_scope`, the same step that finds the
//! places a question names). Matched like place names (`timeline/place.rs::words`: lower case, whole words).
//!
//! **Which words:** only those that in a question nearly always mean where a picture was taken. Words that
//! questions about documents use in another sense stay out or count only with their preposition: "bar" alone
//! is a bar chart, "office" an office lease, "outside" / "binnen" / "внутри" a scope ("outside the EU", "binnen
//! 30 dagen"), Turkish "kar" also profit, French "en train" also "in the process of". A question naming a label
//! never narrows the search: its hits among those pictures are mixed in front of the plain ones.
//!
//! **Term syntax:** lower case words separated by spaces, apostrophes written as a space ("à l intérieur");
//! a trailing `*` lets the last word go on (inflection: "plaż*" finds "plaży", "plażę"). Terms in a script
//! written without spaces between words, or whose words take attached particles or articles (Chinese,
//! Japanese, Korean, Arabic), match anywhere in the text. Tests: `scene_words_tests.rs`.

use crate::timeline::place::words;

/// Label key → the words naming it, all languages in one list (en, de, es, fr, it, nl, pl, pt, ru, tr, vi,
/// zh-Hans/zh-Hant, ja, ko, ar, hi).
const WORDS: [(&str, &[&str]); 13] = [
    ("indoors", &[
        "indoors", "indoor", "inside a building", "drinnen", "in innenräumen", "im innenraum", "en interiores", "adentro",
        "bajo techo", "à l intérieur", "en intérieur", "al chiuso", "all interno", "binnenshuis", "overdekt", "w pomieszczeniu",
        "w pomieszczeniach", "w budynku", "em ambiente fechado", "em local fechado", "lá dentro", "в помещении",
        "в помещениях", "içeride", "kapalı alanda", "kapalı mekanda", "trong nhà", "室内", "室內", "屋里", "屋裡", "屋内",
        "실내", "في الداخل", "داخل المبنى", "घर के अंदर", "कमरे में",
    ]),
    ("outdoors", &[
        "outdoors", "outdoor", "open air", "draußen", "draussen", "im freien", "unter freiem himmel",
        "al aire libre", "afuera", "en exteriores", "en plein air", "à l extérieur", "en extérieur", "all aperto",
        "all esterno", "buitenshuis", "in de open lucht", "na zewnątrz", "na dworze", "na świeżym powietrzu",
        "ao ar livre", "lá fora", "do lado de fora", "на улице", "на открытом воздухе", "снаружи", "dışarıda", "açık havada",
        "açık alanda", "ngoài trời", "户外", "戶外", "室外", "屋外", "野外", "야외", "실외", "الهواء الطلق", "خارج المبنى",
        "खुले में", "खुली हवा", "घर के बाहर",
    ]),
    ("nature", &[
        "in nature", "in the countryside", "in the mountains", "in the woods", "in the forest", "hiking", "in der natur",
        "im wald", "in den bergen", "im grünen", "beim wandern", "en la naturaleza", "en la montaña",
        "en el bosque", "dans la nature", "à la campagne", "en montagne", "à la montagne", "en forêt", "nella natura",
         "in montagna", "nel bosco", "in de natuur", "in het bos", "in de bergen", "na łonie natury",
        "w przyrodzie", "w lesie", "w górach", "na natureza", "na montanha", "na floresta", "на природе",
        "в лесу", "в горах", "doğada", "ormanda", "dağda", "dağlarda", "thiên nhiên", "trong rừng", "trên núi", "大自然",
        "森林", "山里", "山裡", "自然の中", "山で", "숲", "자연 속", "자연에서", "산에서", "الطبيعة", "الغابة", "प्रकृति", "जंगल", "पहाड़ों",
    ]),
    ("city", &[
        "in the city", "in town", "downtown", "city centre", "city center", "in der stadt", "in der innenstadt", "innenstadt",
        "stadtzentrum", "en la ciudad", "centro de la ciudad", "en ville", "dans la ville", "centre ville", "in città",
        "in centro", "in de stad", "in het centrum", "binnenstad", "w mieście", "na cidade", "в городе", "в центре города",
        "şehirde", "şehir merkezinde", "kent merkezinde", "thành phố", "城市里", "城市裡", "市区", "市區", "市中心", "街上",
        "街中", "都会で",   "시내", "도시에서", "المدينة", "शहर",
    ]),
    ("home", &[
        "at home", "at my home", "at our home", "zu hause", "zuhause", "daheim", "in der wohnung", "en casa", "à la maison",
        "chez moi", "chez nous", "a casa", "thuis", "w domu", "em casa", "дома", "evde", "ở nhà", "tại nhà", "在家", "家里",
        "家裡", "自宅", "家で", "집에서", "في المنزل", "في البيت", "घर पर", "घर में",
    ]),
    ("office", &[
        "in the office", "at the office", "in my office", "im büro", "ins büro", "en la oficina", "au bureau",
        "in ufficio", "op kantoor", "op het kantoor", "in het kantoor", "w biurze", "no escritório", "в офисе", 
        "ofiste", "büroda", "ở văn phòng", "tại văn phòng", "在办公室", "在辦公室", "オフィスで", "사무실에서", "في المكتب",
        "दफ़्तर में", "दफ्तर में", "ऑफ़िस में", "ऑफिस में",
    ]),
    ("bar_restaurant", &[
        "in a bar", "at a bar", "in the bar", "at the bar", "in a pub", "in the pub", "at the pub", "pubs", "restaurant*",
        "café", "cafés", "cafe", "cafes", "in der bar", "an der bar", "in einer bar", "in der kneipe", "kneipe", "im lokal",
        "en el bar", "en un bar", "restaurante*", "cafetería*", "au bar", "dans un bar", "au café", "dans un café", "al bar",
        "in un bar", "ristorante", "ristoranti", "in de kroeg", "in een bar", "in de bar", "in het café", "w barze",
        "w pubie", "restauracj*", "w kawiarni", "no bar", "num bar", "em um bar", "no café", "в баре", "в пабе", "ресторан*",
        "в кафе", "barda", "barlarda", "restoran*", "kafede", "lokanta*", "quán bar", "nhà hàng", "quán cà phê", "quán ăn",
        "酒吧", "餐厅", "餐廳", "咖啡馆", "咖啡館", "レストラン", "居酒屋", "カフェ", "술집", "식당", "카페", "레스토랑",
        "مطعم", "مقهى", "حانة", "बार में", "रेस्टोरेंट", "रेस्तरां", "कैफ़े", "कैफे",
    ]),
    ("beach", &[
        "beach*", "strand", "strände", "stränden", "strandes", "playa*", "plage*", "spiaggia", "spiagge", "op het strand",
        "plaż*", "praia*", "пляж*", "plaj*", "sahilde", "kumsalda", "bãi biển", "海滩", "海灘", "沙滩", "沙灘", "ビーチ",
        "砂浜", "해변", "해수욕장", "الشاطئ", "شاطئ", "समुद्र तट", "बीच पर",
    ]),
    ("water", &[
        "by the water", "on the water", "at the lake", "by the lake", "on the lake", "by the sea", "at the sea",
        "by the river", "on the river", "at the river", "lakeside", "riverside", "seaside", "waterfront", "am wasser",
        "auf dem wasser", "am see", "auf dem see", "am meer", "am fluss", "an der see", "am ufer", "junto al agua",
        "en el lago", "junto al lago", "en el río", "junto al río", "junto al mar", "au bord de l eau", "au bord du lac",
        "au bord de la mer", "au bord de la rivière", "sur le lac", "al lago", "sul lago", "al fiume", "al mare",
        "sull acqua", "in riva", "aan het water", "op het water", "aan het meer", "aan de rivier", "aan zee", "aan de zee",
        "nad wodą", "nad jeziorem", "nad rzeką", "nad morzem", "à beira", "na beira", "beira mar", "no lago", "no rio",
        "у воды", "на озере", "у озера", "на реке", "у реки", "на море", "у моря", "su kenarında", "göl kenarında",
        "gölde", "nehir kenarında", "deniz kenarında", "bờ hồ", "bên hồ", "bờ sông", "bên sông", "bờ biển",
        "trên mặt nước", "水边", "水邊", "湖边", "湖邊", "河边", "河邊", "海边", "海邊", "水辺", "湖で", "川で", "海辺",
        "물가", "호숫가", "강가", "바닷가", "البحيرة", "النهر", "البحر", "झील", "नदी", "समुद्र",
    ]),
    ("vehicle", &[
        "in the car", "in a car", "in my car", "on the train", "on a train", "in the train", "on the bus", "on a bus",
        "on the plane", "on a plane", "on the boat", "on a boat", "im auto", "im zug", "im bus", "im flugzeug", "im boot",
        "auf dem boot", "im wagen", "en el coche", "en el auto", "en el carro", "en el tren", "en el autobús", "en el avión",
        "en el barco", "en voiture", "dans la voiture", "dans le train", "dans le bus", "en avion", "dans l avion",
        "en bateau", "sur le bateau", "in macchina", "in treno", "in autobus", "in aereo", "in barca", "in de auto",
        "in de trein", "in de bus", "in het vliegtuig", "op de boot", "w samochodzie", "w aucie", "w pociągu",
        "w autobusie", "w samolocie", "na łodzi", "na statku", "no carro", "no trem", "no ônibus", "no avião", "no barco",
        "в машине", "в автомобиле", "в поезде", "в автобусе", "в самолёте", "в самолете", "на лодке", "на корабле",
        "arabada", "trende", "otobüste", "uçakta", "teknede", "gemide", "trong xe", "trên xe", "tàu hỏa", "xe buýt",
        "máy bay", "trên thuyền", "车里", "車裡", "车上", "車上", "火车", "火車", "飞机上", "飛機上", "船上", "車内", "車の中",
        "電車", "飛行機", "차 안", "기차", "비행기", "버스", "السيارة", "القطار", "الطائرة", "الحافلة", "القارب",
        "कार में", "गाड़ी में", "ट्रेन", "बस में", "हवाई जहाज़", "नाव",
    ]),
    ("sport", &[
        "sport*", "stadium", "at the gym", "in the gym", "stadion", "im fitnessstudio", "deporte*", "deportiv*", "estadio*",
        "en el gimnasio", "à la salle de sport", "stadio", "au stade", "dans le stade", "in palestra", "in de sportschool", "na stadionie",
        "na siłowni", "esporte*", "estádio*", "спорт*", "на стадионе", "в спортзале", "spor", "sporda",
        "sporu", "stadyumda", "spor salonunda", "thể thao", "sân vận động", "phòng gym",    
        "健身房", "运动场", "運動場", "体育场", "體育場", "体育馆", "體育館", "スポーツ", "ジム", "スタジアム", "스포츠", "운동장", "체육관", "경기장", "رياضة", "ملعب", "खेल",
        "स्टेडियम", "जिम",
    ]),
    ("snow", &[
        "snow*", "schnee*", "verschneit*", "nieve", "nevad*", "neige*", "enneig*", "neve", "innevat*", "sneeuw*",
        "śnieg*", "w śniegu", "снег*", "в снегу", "karda", "karla kaplı", "kar manzarası", "tuyết", "雪", "눈밭", "설경",
        "눈 내린", "ثلج", "الثلوج", "बर्फ",
    ]),
    ("stage", &[
        "on stage", "on the stage", "concert", "concerts", "auf der bühne", "bühne", "konzert*", "en el escenario", "sobre el escenario",
        "concierto*", "sur scène", "sur la scène", "sul palco", "palcoscenico", "concerto", "concerti", "op het podium",
        "op het toneel", "na scenie", "koncert*", "koncercie", "no palco", "на сцене", "концерт*", "sahnede", "konser", "konserde", "konserler", "sân khấu",
        "buổi hòa nhạc", "舞台", "演唱会", "演唱會", "音乐会", "音樂會", "ステージ", "コンサート", "ライブ会場", "무대", "공연",
        "콘서트", "المسرح", "على المنصة", "حفلة", "मंच", "स्टेज",
    ]),
];

/// Whether `term` is in a script whose terms match anywhere (see the module comment).
fn anywhere(term: &str) -> bool {
    term.chars().any(|c| matches!(c as u32, 0x0600..=0x06FF | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF))
}

/// Whether `term` stands in `hay` (already [`words`]).
fn stands_in(hay: &str, term: &str) -> bool {
    let (term, open) = match term.strip_suffix('*') {
        Some(stem) => (stem, true),
        None => (term, false),
    };
    let w = words(term);
    if anywhere(term) {
        hay.contains(w.trim())
    } else if open {
        hay.contains(w.trim_end())
    } else {
        hay.contains(&w)
    }
}

/// The label keys `text` names, in the order of `scene_labels::keys`; empty when it names none.
pub fn named(text: &str) -> Vec<&'static str> {
    let hay = words(text);
    WORDS.iter().filter(|(_, terms)| terms.iter().any(|t| stands_in(&hay, t))).map(|(key, _)| *key).collect()
}

#[cfg(test)]
#[path = "scene_words_tests.rs"]
mod tests;
