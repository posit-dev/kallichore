# McpFrontendRegistration

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**frontend_id** | **String** | A previously issued frontend ID. Omit to have the server generate one. | [optional] [default to None]
**display_name** | **String** | A human-readable name for the frontend, shown in logs and status | 
**preferred_port** | **i32** | The TCP port the MCP listener should bind. Used only when the listener isn't running yet, and ignored when the port is unavailable. | [optional] [default to None]
**capabilities** | [***models::McpFrontendCapabilities**](mcpFrontendCapabilities.md) |  | [optional] [default to None]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


