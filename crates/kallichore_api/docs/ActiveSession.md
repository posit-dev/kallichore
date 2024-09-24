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
**working_directory** | **String** | The session's current working directory | 
**execution_queue** | [***models::ExecutionQueue**](execution_queue.md) |  | 
**status** | [***models::Status**](status.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


