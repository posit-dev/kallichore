# McpWorkspaceRegistration

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**workspace_id** | **String** | A previously issued workspace ID. Omit to have the server generate one from the display name. | [optional] [default to None]
**display_name** | **String** | A human-readable name for the workspace, normally the folder the user has open. Shown in logs and status, and used to build the workspace ID. | 
**preferred_port** | **i32** | The TCP port the MCP listener should bind. Used only when the listener isn't running yet, and ignored when the port is unavailable. | [optional] [default to None]
**token** | **String** | A bearer token the server issued for this workspace before. Supplying it again keeps the token agents are configured with valid across a restart of the server, which holds no state of its own. Omit it to have the server issue one, and ignored unless it is well formed. | [optional] [default to None]
**capabilities** | [***models::McpWorkspaceCapabilities**](mcpWorkspaceCapabilities.md) |  | [optional] [default to None]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


