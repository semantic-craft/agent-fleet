use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BriefPolicyError {
    ModelWrongGate,
    ModelSelfAttestation,
}

impl fmt::Display for BriefPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::ModelWrongGate => "MODEL_WRONG gates are forbidden",
            Self::ModelSelfAttestation => "worker model self-attestation is forbidden",
        };
        write!(
            f,
            "brief policy rejected: {reason}; configure models in the dispatcher"
        )
    }
}

impl std::error::Error for BriefPolicyError {}

/// Reject reserved gate language even inside quotes or negated instructions.
/// This lexical boundary is not a general natural-language intent classifier.
pub fn validate_brief(text: &str) -> Result<(), BriefPolicyError> {
    let folded = text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let compact: String = folded.chars().filter(|c| c.is_alphanumeric()).collect();
    if compact.contains("modelwrong") {
        return Err(BriefPolicyError::ModelWrongGate);
    }
    let explicit = [
        "modelselfattest",
        "selfattestmodel",
        "selfattestyourmodel",
        "模型自证",
        "自证模型",
        "自报模型",
        "自报你的模型",
        "自报当前模型",
    ];
    if explicit.iter().any(|pattern| compact.contains(pattern)) {
        return Err(BriefPolicyError::ModelSelfAttestation);
    }

    for clause in folded.split(['.', ';', '!', '?', '\n', '。', '；', '！', '？']) {
        let words: Vec<&str> = clause
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let has = |word| words.contains(&word);
        let reports_identity = [
            "attest", "confirm", "verify", "prove", "state", "report", "print", "return",
            "identify", "declare", "certify", "output",
        ]
        .iter()
        .any(|word| has(word));
        let own_model = clause.contains("your model")
            || clause.contains("your actual model")
            || clause.contains("your current model")
            || clause.contains("your underlying model")
            || clause.contains("model you are")
            || clause.contains("model you're")
            || clause.contains("which model")
            || clause.contains("what model");
        let chinese_report = [
            "证明", "确认", "报告", "声明", "输出", "返回", "回答", "核实", "验证",
        ]
        .iter()
        .any(|word| clause.contains(word));
        let chinese_own_model = clause.contains("模型")
            && [
                "你是", "你用", "你的", "当前", "实际", "自身", "自己", "身份",
            ]
            .iter()
            .any(|word| clause.contains(word));
        let named_model = [
            "gpt", "claude", "gemini", "deepseek", "grok", "qwen", "llama", "minimax", "glm",
            "zcode",
        ]
        .iter()
        .any(|name| words.iter().any(|word| word.starts_with(name)));
        let named_identity = named_model
            && (clause.contains("you are") || clause.contains("you're") || clause.contains("你是"));
        let first_line_identity = (clause.contains("first line")
            && (own_model
                || clause.contains("model name")
                || clause.contains("model identity")
                || clause.contains("model id")))
            || (clause.contains("第一行") && clause.contains("模型"));
        if (reports_identity && (own_model || named_identity))
            || (chinese_report && (chinese_own_model || named_identity))
            || first_line_identity
        {
            return Err(BriefPolicyError::ModelSelfAttestation);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_policy_rejects_model_wrong_gate_variants() {
        for text in [
            "If you are not the requested model, return MODEL_WRONG and stop.",
            "不符合就输出 model_wrong。",
            "Return MoDeL-WrOnG",
            "Return MODEL\nWRONG",
            "Return MODEL wrong",
            "Quoted example: `MODEL_WRONG`",
            "Do not return MODEL_WRONG",
        ] {
            assert_eq!(
                validate_brief(text),
                Err(BriefPolicyError::ModelWrongGate),
                "{text}"
            );
        }
    }

    #[test]
    fn brief_policy_rejects_english_self_attestation() {
        for text in [
            "First confirm your model is gpt-6.",
            "Before starting, report your actual model and provider.",
            "State which model you are running.",
            "Print the model you're using.",
            "Please self-attest your model.",
            "Model self attestation must pass before work.",
            "Verify your current model identity, then continue.",
            "Before starting confirm you are GPT-6.",
            "First line: model name.",
            "Report\n\tyour model before beginning.",
        ] {
            assert_eq!(
                validate_brief(text),
                Err(BriefPolicyError::ModelSelfAttestation),
                "{text}"
            );
        }
    }

    #[test]
    fn brief_policy_rejects_chinese_self_attestation() {
        for text in [
            "先自报模型，再开始执行。",
            "先证明你是指定的模型。",
            "请确认当前模型，符合再继续。",
            "返回你的模型名称。",
            "输出实际模型身份，否则停止。",
            "模型自证是前置条件。",
            "先确认你是 GPT-6，再开始执行。",
            "先确认你是 GLM-5.3-Flash，再开始执行。",
            "第一行写出模型名称。",
        ] {
            assert_eq!(
                validate_brief(text),
                Err(BriefPolicyError::ModelSelfAttestation),
                "{text}"
            );
        }
    }

    #[test]
    fn brief_policy_allows_task_text_and_dispatcher_configuration() {
        for text in [
            "Fix delivery receipts and run cargo test --workspace.",
            "实现收据状态，测试通过后回报真实输出。",
            "The dispatcher configures the model. Read src/lib.rs and fix the bug.",
            "Model configuration belongs to dispatcher metadata. Do the task.",
            "模型配置由 dispatcher 管理。只检查投递收据。",
            "Review the data model and report test failures.",
            "Verify model loading code against the fixture.",
            "Verify the model name parser and report test results.",
            "Verify model identity serialization code against the fixture.",
            "Report model id parser test failures.",
        ] {
            assert_eq!(validate_brief(text), Ok(()), "{text}");
        }
    }

    #[test]
    fn brief_policy_rejection_does_not_echo_brief_contents() {
        let error = validate_brief("secret input; confirm your model").unwrap_err();
        assert!(!error.to_string().contains("secret input"));
        assert!(error.to_string().contains("dispatcher"));
    }
}
