# ActiveSession

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**session_id** | **String** | A unique identifier for the session | 
**argv** | **Vec<String>** | The program and command-line parameters for the session | 
**process_id** | **i32** | The underlying process ID of the session, if the session is running. | [optional] [default to None]
**username** | **String** | The username of the user who owns the session | 
**display_name** | **String** | A human-readable name for the session | 
**language** | **String** | The interpreter language | 
**interrupt_mode** | [***models::InterruptMode**](interrupt_mode.md) |  | 
**initial_env** | **std::collections::HashMap<String, String>** | The environment variables set when the session was started | [optional] [default to None]
**connected** | **bool** | Whether the session is connected to a client | 
**started** | [**chrono::DateTime::<chrono::Utc>**](DateTime.md) | An ISO 8601 timestamp of when the session was started | 
**session_mode** | [***models::SessionMode**](sessionMode.md) |  | 
**working_directory** | **String** | The session's current working directory | 
**notebook_uri** | **String** | For notebook sessions, the URI of the notebook file | [optional] [default to None]
**input_prompt** | **String** | The text to use to prompt for input | 
**continuation_prompt** | **String** | The text to use to prompt for input continuations | 
**execution_queue** | [***models::ExecutionQueue**](execution_queue.md) |  | 
**status** | [***models::Status**](status.md) |  | 
**kernel_info** | [***serde_json::Value**](.md) | The kernel information, as returned by the kernel_info_request message | 
**idle_seconds** | **i32** | The number of seconds the session has been idle, or 0 if the session is busy | 
**busy_seconds** | **i32** | The number of seconds the session has been busy, or 0 if the session is idle | 
**socket_path** | **String** | The path to the Unix domain socket used to send/receive data from the session, if applicable | [optional] [default to None]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


