# ExecuteOutput

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**r#type** | [***models::ExecuteOutputType**](executeOutput_type.md) |  | 
**stream_name** | **String** | The stream name (stdout or stderr), for stream output | [optional] [default to None]
**text** | **String** | The text content, for stream output | [optional] [default to None]
**data** | **std::collections::HashMap<String, String>** | MIME-keyed data, for display_data output | [optional] [default to None]
**metadata** | [***serde_json::Value**](.md) | Metadata dictionary, for display_data output | [optional] [default to None]
**error_name** | **String** | The error name, for error output | [optional] [default to None]
**error_message** | **String** | The error message, for error output | [optional] [default to None]
**error_traceback** | **Vec<String>** | The error traceback lines, for error output | [optional] [default to None]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


