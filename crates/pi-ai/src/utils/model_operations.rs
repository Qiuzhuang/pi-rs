//! Model type helpers and error results (port of `utils/model-operations.ts`).

use super::models_error::{ModelsError, ModelsErrorCode};
use crate::types::{
    AnyModel, AssistantImages, ClassifierContext, ClassifierModel, ClassifierResult, ClassifierStopReason,
    ImageModel, ImagesStopReason, Model, ModelInputModality, ModelType,
};

/// The type of a model. Models without `type` are chat models.
pub fn get_model_type(model: &AnyModel) -> ModelType {
    model.model_type()
}

/// Runtime model type check.
pub fn is_model_type(model: &AnyModel, model_type: ModelType) -> bool {
    get_model_type(model) == model_type
}

/// Whether a chat `Model` value is actually typed as chat (`type` missing or `"chat"`).
pub fn is_chat_model(model: &Model) -> bool {
    model.model_type.as_deref().is_none_or(|t| t == "chat")
}

pub fn assert_chat_model(model: &Model) -> Result<(), ModelsError> {
    if !is_chat_model(model) {
        return Err(ModelsError::new(
            ModelsErrorCode::Provider,
            format!("Model {}/{} is not a chat model", model.provider, model.id),
        ));
    }
    Ok(())
}

pub fn assert_image_model(model: &ImageModel) -> Result<(), ModelsError> {
    if model.model_type != "image" {
        return Err(ModelsError::new(
            ModelsErrorCode::Provider,
            format!("Model {}/{} is not an image model", model.provider, model.id),
        ));
    }
    Ok(())
}

pub fn assert_classifier_model(model: &ClassifierModel) -> Result<(), ModelsError> {
    if model.model_type != "classifier" {
        return Err(ModelsError::new(
            ModelsErrorCode::Provider,
            format!("Model {}/{} is not a classifier model", model.provider, model.id),
        ));
    }
    Ok(())
}

/// Rejects classifier images for models whose catalog entry does not accept image input.
pub fn assert_classifier_input_supported(model: &ClassifierModel, context: &ClassifierContext) -> Result<(), ModelsError> {
    if context.images.as_ref().is_some_and(|i| !i.is_empty()) && !model.input.contains(&ModelInputModality::Image) {
        return Err(ModelsError::new(
            ModelsErrorCode::Provider,
            format!("Model {}/{} does not accept image input", model.provider, model.id),
        ));
    }
    Ok(())
}

pub fn image_error_result(model: &ImageModel, error: &dyn std::fmt::Display, aborted: bool) -> AssistantImages {
    AssistantImages {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        output: Vec::new(),
        response_id: None,
        usage: None,
        stop_reason: if aborted { ImagesStopReason::Aborted } else { ImagesStopReason::Error },
        error_message: Some(error.to_string()),
        timestamp: super::now_ms(),
    }
}

pub fn classifier_error_result(model: &ClassifierModel, error: &dyn std::fmt::Display, aborted: bool) -> ClassifierResult {
    ClassifierResult {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        answers: Default::default(),
        usage: None,
        stop_reason: if aborted { ClassifierStopReason::Aborted } else { ClassifierStopReason::Error },
        error_message: Some(error.to_string()),
        timestamp: super::now_ms(),
    }
}
