# ExecutionHistoryEntry

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**input** | **String** | The code that was run | 
**output** | **String** | The text the code produced: standard output and error, displays, and the result, in the order they arrived | 
**error** | [***models::ExecutionError**](executionError.md) |  | [optional] [default to None]
**timestamp** | **i64** | A Unix timestamp in milliseconds indicating when the code was sent to the kernel | 
**source** | **String** | What submitted the code, when known, such as 'agent', 'interactive', or 'script' | [optional] [default to None]
**agent** | **String** | The name of the agent that submitted the code, when it was an agent | [optional] [default to None]
**truncated** | **bool** | Whether any part of the entry was clipped | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


