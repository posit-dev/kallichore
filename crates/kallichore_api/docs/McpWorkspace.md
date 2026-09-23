# McpWorkspace

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**workspace_id** | **String** | The workspace's ID; supply it again to re-register after a reconnect | 
**token** | **String** | The bearer token agents present to the MCP server. Scoped to this workspace and distinct from the supervisor API token. | 
**port** | **i32** | The TCP port the MCP listener is bound to on 127.0.0.1 | 
**url** | **String** | The full MCP endpoint URL agents should connect to. Unique to this workspace, so an agent configured with it can only reach this workspace's sessions. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


