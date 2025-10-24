# ServerStatus

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**sessions** | **i32** |  | 
**active** | **i32** |  | 
**busy** | **bool** |  | 
**idle_seconds** | **i32** | The number of seconds all sessions have been idle, or 0 if any session is busy | 
**busy_seconds** | **i32** | The number of seconds any session has been busy, or 0 if all sessions are idle | 
**uptime_seconds** | **i32** | The number of seconds the server has been running | 
**version** | **String** | The version of the server | 
**process_id** | **i32** | The server's operating system process identifier | 
**started** | [**chrono::DateTime::<chrono::Utc>**](DateTime.md) | An ISO 8601 timestamp of when the server was started | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


