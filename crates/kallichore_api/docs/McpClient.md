# McpClient

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **i32** | Identifies the client within its workspace | 
**name** | **String** | The agent's name, from the MCP clientInfo | [optional] [default to None]
**version** | **String** | The agent's version, from the MCP clientInfo | [optional] [default to None]
**pid** | **i32** | The process ID of the bridge | [optional] [default to None]
**working_directory** | **String** | The bridge's working directory, normally the agent's | [optional] [default to None]
**session_id** | **String** | The session the client is running inside, for a client in a kernel | [optional] [default to None]
**connected_at** | [**chrono::DateTime::<chrono::Utc>**](DateTime.md) | When the client connected | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


