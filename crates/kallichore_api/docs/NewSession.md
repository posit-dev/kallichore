# NewSession

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**session_id** | **String** | A unique identifier for the session | 
**display_name** | **String** | A human-readable name for the session | 
**language** | **String** | The interpreter language | 
**username** | **String** | The username of the user who owns the session | 
**input_prompt** | **String** | The text to use to prompt for input | 
**continuation_prompt** | **String** | The text to use to prompt for input continuations | 
**argv** | **Vec<String>** | The program and command-line parameters for the session | 
**working_directory** | **String** | The working directory in which to start the session. | 
**env** | **std::collections::HashMap<String, String>** | Environment variables to set for the session | 
**connection_timeout** | **i32** | The number of seconds to wait for a connection to the session's ZeroMQ sockets before timing out | [optional] [default to Some(30)]
**interrupt_mode** | [***models::InterruptMode**](interrupt_mode.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


