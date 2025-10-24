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
**session_mode** | [***models::SessionMode**](sessionMode.md) |  | 
**working_directory** | **String** | The working directory in which to start the session. | 
**notebook_uri** | **String** | For notebook sessions, the URI of the notebook file | [optional] [default to None]
**env** | [**Vec<models::VarAction>**](varAction.md) | A list of environment variable actions to perform | 
**connection_timeout** | **i32** | The number of seconds to wait for a connection to the session's ZeroMQ sockets before timing out | [optional] [default to Some(30)]
**interrupt_mode** | [***models::InterruptMode**](interrupt_mode.md) |  | 
**protocol_version** | **String** | The Jupyter protocol version supported by the underlying kernel | [optional] [default to Some("5.3".to_string())]
**run_in_shell** | **bool** | Whether to run the session inside a login shell; only relevant on POSIX systems | [optional] [default to Some(false)]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


