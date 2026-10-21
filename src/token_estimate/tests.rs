//! The estimate against real token counts: short texts of the calibration set (`token_estimate.rs`
//! module comment) with the larger of Apple's on-device and Qwen3's count, measured 2026-10-21.

use super::*;

/// `(language, real tokens, text)`.
const MEASURED: &[(&str, usize, &str)] = &[
    ("de/doc", 49, "# Briefkastengesellschaft\n\nQuelle: Wikipedia (de), Artikel „Briefkastengesellschaft\", Lizenz CC BY-SA 4.0, abgerufen 2026-10-20."),
    ("en/doc", 55, "13\nengines.  He said that he advised the port authority so they could have the emergency\nequipment standing by.  He did this through the Emergency Alert Notification System,\nthe red crash phone that was located in the tower.  He said that these phone conversations"),
    ("ar", 108, "جارٍ إعادة فحص الاقتباسات…\nكائن واحد لكل مقطع (إضافة أعمدة)\nاقتباس في الإجابة غير موجود في المصدر المستشهد به — يُطلب من النموذج المحاولة مرة أخرى.\nإضافة مفهوم جديد إلى الرسم البياني\nدمج تلقائي للأزواج عالية الثقة دون LLM (إزالة تكرار الرسم البياني)\nيجيب بنماذج خادم Ollama الخاص بك."),
    ("hi", 235, "जैसे ऐसा एज जिसके स्रोत नोड का कोई नाम नहीं।\nछवियों का वर्णन हो रहा है…\nफ़ाइल  MB की सीमा से बड़ी है।\n other } ·  other }\nआकार का अनुमान नहीं लगाया जा सका।\nदस्तावेज़ जोड़ें, फिर वे इंडेक्स हो जाते हैं।\nसंदर्भ सेट “” इंपोर्ट हो गया।\n— डाउनलोड किया हुआ ONNX मॉडल चुनें —"),
    ("ja", 194, "が画像から作成したもので、出典のテキストではありません\n画像検索の導入前に追加された書類にはページ画像がありません。図版をインデックス作成するには、書類を再度追加してください。\nコンテキスト上限（トークン）：\nモデルがのような内部推論タグを出力するかどうかを指定します。推論の手順を含められるよう、厳格な出力構造を調整します。\n読み込み、分割、インデックス作成中… /\n ヒント：正規表現に不慣れな場合は、書類の数行（例：「§ 4 Abs. 1」）をコピーして、ChatGPTやClaudeに次のように尋ねてください：「この種の見出しに一致する短い正規表現を書いてください。書類の分割に使います。」"),
    ("ko", 157, "— }\n정답 세트는 정답 앵커를 알고 있는 질문을 모아 둔 목록으로, LLM 없이 검색 품질을 측정하는 데 쓰입니다. 앵커는 해당 질문에 답해야 하는 코퍼스 구절에 글자 그대로 들어 있어야 하는 부분 문자열이며, 항목의 앵커 중 하나라도 포함하면 그 구절은 관련 있는 것으로 집계됩니다. 앵커를 “&&”로 연결하면(예: “150 Mio && 25 %”) 하나만이 아니라 여러 부분 문자열이 모두 같은 구절에 있어야 하도록 지정할 수 있습니다.\n지식 그래프(}, })"),
    ("pl", 75, "Ustawienia odpowiedzi — \nEwaluacja ocenia każdy wpis wybranego zestawu wzorcowego względem wybranych kolekcji.\nZgodny z OpenAI / hostowany (np. Groq)\nnp. endpoint zwrócił błąd dekodowania, gdy model lokalny był obciążony."),
    ("ru", 60, "outgoing_refs в том виде, в каком они переданы модели с этим совпадением — разрешённые ID можно получить через get_chunks, для неразрешённых отсылок нужен lookup_exact\nПроверить много документов сразу\n few  many  other }"),
    ("vi", 90, "Khuyến nghị: Xóa — một cạnh không có căn cứ là rủi ro trong đồ thị tuân thủ. Dùng “Xóa tất cả” để dọn toàn bộ phần tồn đọng một lần. Chỉ Giữ nếu quan hệ rõ ràng đúng theo đồ thị.\n /  nút đã xong\nCác lượt trước được gửi kèm mỗi câu hỏi; mỗi lượt được lưu trong Cuộc trò chuyện gần đây."),
    ("zh-Hans", 209, "项本地匹配 ·  项由模型恢复 ·  项无法定位 ·  项调用失败（可重试）\nMatrix 会一次性下载搜索模型（），以便搜索你的文档。下载在后台进行。\nØ  对/秒（最近一分钟：/秒）· 上一批：无匹配\n 个失败）}}\n 项已确认 ·  项被推翻（已移除边）·  项现为不明确（移至极性）·  项仍失败 ·  项已剔除\n来源：德语维基百科文章（2026-10-20 获取）。作者：请查看各文章的版本历史。\n重排序需要一个已启用的重排序模型——设置 → 模型 →“重排序模型”：添加一个（本地模型目录，或兼容 Jina/Cohere/TEI 的远程端点）并将其设为启用。没有启用的模型时，运行会出错。"),
    ("table", 129, "15:26:36.35 17.84 755 N 40° 51' 01.32\" W 073° 52' 16.91\" 0.58 4.87\n15:26:45.56 17.70 749 N 40° 51' 08.50\" W 073° 52' 05.13\" 0.85 4.95"),
];

#[test]
fn never_far_below_and_not_wasteful_against_measured_counts() {
    for &(lang, real, text) in MEASURED {
        let est = estimate_tokens(text);
        let ratio = est as f64 / real as f64;
        assert!((1.0..=1.4).contains(&ratio), "{lang}: estimate {est} vs real {real} ({ratio:.2})");
    }
}

#[test]
fn the_old_flat_estimate_was_far_below_for_these() {
    // chars/4 × 1.2 (before): what made German and non-Latin prompts overflow the window.
    let old = |t: &str| (t.chars().count().div_ceil(4) as f64 * 1.2) as usize;
    for (lang, max_ratio) in [("hi", 0.4), ("ja", 0.5), ("table", 0.35), ("ar", 0.8)] {
        let &(_, real, text) = MEASURED.iter().find(|m| m.0 == lang).unwrap();
        assert!((old(text) as f64) < max_ratio * real as f64, "{lang}");
    }
}

#[test]
fn english_words_are_cheaper_than_the_same_letters_in_other_languages() {
    let en = "The report says that the crew of the aircraft followed the checklist with care.";
    let de = "Der Bericht besagt, dass die Besatzung des Flugzeugs die Checkliste sorgfältig befolgte.";
    assert!(is_english(en, &runs(en)));
    assert!(!is_english(de, &runs(de)));
    // Dutch shares "is"/"of"/"-ing" with English but not its function words' density.
    let nl = "De verwerking van het document is klaar; de samenvatting staat in de lijst of in het archief.";
    assert!(!is_english(nl, &runs(nl)));
    let per_char = |t: &str| estimate_tokens(t) as f64 / t.chars().count() as f64;
    assert!(per_char(en) < per_char(de));
}

#[test]
fn digits_and_the_space_before_them_count_one_each() {
    assert_eq!(estimate_tokens("2026"), 4);
    assert_eq!(estimate_tokens("a 1"), 3); // word, space before a digit, digit
    assert_eq!(estimate_tokens("a b"), 2); // a space before a word joins it
}

#[test]
fn empty_and_tiny_texts_cost_at_least_one() {
    assert_eq!(estimate_tokens(""), 1);
    assert_eq!(estimate_tokens(" "), 1);
    assert_eq!(estimate_tokens("ok"), 1);
}

#[test]
fn longer_text_never_costs_less() {
    let text = "Die Gesellschaft wurde 2016 in Panama gegründet — 名前 — имя — اسم — नाम.";
    let mut prev = 0;
    for (i, _) in text.char_indices() {
        let t = estimate_tokens(&text[..i]);
        assert!(t >= prev, "at {i}: {t} < {prev}");
        prev = t;
    }
}
