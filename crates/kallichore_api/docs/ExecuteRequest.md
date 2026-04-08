# ExecuteRequest

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**code** | **String** | The code to execute | 
**silent** | **bool** | If true, signals the kernel to execute quietly: no broadcast on iopub, no execute_result, and the execution_count is not incremented. Defaults to false. | [optional] [default to Some(false)]
**store_history** | **bool** | If true (default), the code is stored in the kernel's history. Set to false for throwaway executions. | [optional] [default to Some(true)]
**stop_on_error** | **bool** | If true (default), abort the execution queue on error. If false, queued execute requests will still be processed even if this one fails. | [optional] [default to Some(true)]
**timeout_seconds** | **i32** | Maximum number of seconds to wait for execution to complete. If not specified, the request will block indefinitely until execution finishes. | [optional] [default to None]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


