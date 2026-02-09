package com.astraeadb.grpc.proto;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@javax.annotation.Generated(
    value = "by gRPC proto compiler (version 1.72.0)",
    comments = "Source: astraea.proto")
@io.grpc.stub.annotations.GrpcGenerated
public final class AstraeaServiceGrpc {

  private AstraeaServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "astraea.AstraeaService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.CreateNodeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getCreateNodeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateNode",
      requestType = com.astraeadb.grpc.proto.CreateNodeRequest.class,
      responseType = com.astraeadb.grpc.proto.MutationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.CreateNodeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getCreateNodeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.CreateNodeRequest, com.astraeadb.grpc.proto.MutationResponse> getCreateNodeMethod;
    if ((getCreateNodeMethod = AstraeaServiceGrpc.getCreateNodeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getCreateNodeMethod = AstraeaServiceGrpc.getCreateNodeMethod) == null) {
          AstraeaServiceGrpc.getCreateNodeMethod = getCreateNodeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.CreateNodeRequest, com.astraeadb.grpc.proto.MutationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateNode"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.CreateNodeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.MutationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("CreateNode"))
              .build();
        }
      }
    }
    return getCreateNodeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.GetNodeRequest,
      com.astraeadb.grpc.proto.GetNodeResponse> getGetNodeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetNode",
      requestType = com.astraeadb.grpc.proto.GetNodeRequest.class,
      responseType = com.astraeadb.grpc.proto.GetNodeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.GetNodeRequest,
      com.astraeadb.grpc.proto.GetNodeResponse> getGetNodeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.GetNodeRequest, com.astraeadb.grpc.proto.GetNodeResponse> getGetNodeMethod;
    if ((getGetNodeMethod = AstraeaServiceGrpc.getGetNodeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getGetNodeMethod = AstraeaServiceGrpc.getGetNodeMethod) == null) {
          AstraeaServiceGrpc.getGetNodeMethod = getGetNodeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.GetNodeRequest, com.astraeadb.grpc.proto.GetNodeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetNode"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.GetNodeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.GetNodeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("GetNode"))
              .build();
        }
      }
    }
    return getGetNodeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.UpdateNodeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getUpdateNodeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "UpdateNode",
      requestType = com.astraeadb.grpc.proto.UpdateNodeRequest.class,
      responseType = com.astraeadb.grpc.proto.MutationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.UpdateNodeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getUpdateNodeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.UpdateNodeRequest, com.astraeadb.grpc.proto.MutationResponse> getUpdateNodeMethod;
    if ((getUpdateNodeMethod = AstraeaServiceGrpc.getUpdateNodeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getUpdateNodeMethod = AstraeaServiceGrpc.getUpdateNodeMethod) == null) {
          AstraeaServiceGrpc.getUpdateNodeMethod = getUpdateNodeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.UpdateNodeRequest, com.astraeadb.grpc.proto.MutationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "UpdateNode"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.UpdateNodeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.MutationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("UpdateNode"))
              .build();
        }
      }
    }
    return getUpdateNodeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.DeleteNodeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getDeleteNodeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteNode",
      requestType = com.astraeadb.grpc.proto.DeleteNodeRequest.class,
      responseType = com.astraeadb.grpc.proto.MutationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.DeleteNodeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getDeleteNodeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.DeleteNodeRequest, com.astraeadb.grpc.proto.MutationResponse> getDeleteNodeMethod;
    if ((getDeleteNodeMethod = AstraeaServiceGrpc.getDeleteNodeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getDeleteNodeMethod = AstraeaServiceGrpc.getDeleteNodeMethod) == null) {
          AstraeaServiceGrpc.getDeleteNodeMethod = getDeleteNodeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.DeleteNodeRequest, com.astraeadb.grpc.proto.MutationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteNode"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.DeleteNodeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.MutationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("DeleteNode"))
              .build();
        }
      }
    }
    return getDeleteNodeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.CreateEdgeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getCreateEdgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateEdge",
      requestType = com.astraeadb.grpc.proto.CreateEdgeRequest.class,
      responseType = com.astraeadb.grpc.proto.MutationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.CreateEdgeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getCreateEdgeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.CreateEdgeRequest, com.astraeadb.grpc.proto.MutationResponse> getCreateEdgeMethod;
    if ((getCreateEdgeMethod = AstraeaServiceGrpc.getCreateEdgeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getCreateEdgeMethod = AstraeaServiceGrpc.getCreateEdgeMethod) == null) {
          AstraeaServiceGrpc.getCreateEdgeMethod = getCreateEdgeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.CreateEdgeRequest, com.astraeadb.grpc.proto.MutationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateEdge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.CreateEdgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.MutationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("CreateEdge"))
              .build();
        }
      }
    }
    return getCreateEdgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.GetEdgeRequest,
      com.astraeadb.grpc.proto.GetEdgeResponse> getGetEdgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetEdge",
      requestType = com.astraeadb.grpc.proto.GetEdgeRequest.class,
      responseType = com.astraeadb.grpc.proto.GetEdgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.GetEdgeRequest,
      com.astraeadb.grpc.proto.GetEdgeResponse> getGetEdgeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.GetEdgeRequest, com.astraeadb.grpc.proto.GetEdgeResponse> getGetEdgeMethod;
    if ((getGetEdgeMethod = AstraeaServiceGrpc.getGetEdgeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getGetEdgeMethod = AstraeaServiceGrpc.getGetEdgeMethod) == null) {
          AstraeaServiceGrpc.getGetEdgeMethod = getGetEdgeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.GetEdgeRequest, com.astraeadb.grpc.proto.GetEdgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetEdge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.GetEdgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.GetEdgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("GetEdge"))
              .build();
        }
      }
    }
    return getGetEdgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.UpdateEdgeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getUpdateEdgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "UpdateEdge",
      requestType = com.astraeadb.grpc.proto.UpdateEdgeRequest.class,
      responseType = com.astraeadb.grpc.proto.MutationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.UpdateEdgeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getUpdateEdgeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.UpdateEdgeRequest, com.astraeadb.grpc.proto.MutationResponse> getUpdateEdgeMethod;
    if ((getUpdateEdgeMethod = AstraeaServiceGrpc.getUpdateEdgeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getUpdateEdgeMethod = AstraeaServiceGrpc.getUpdateEdgeMethod) == null) {
          AstraeaServiceGrpc.getUpdateEdgeMethod = getUpdateEdgeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.UpdateEdgeRequest, com.astraeadb.grpc.proto.MutationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "UpdateEdge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.UpdateEdgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.MutationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("UpdateEdge"))
              .build();
        }
      }
    }
    return getUpdateEdgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.DeleteEdgeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getDeleteEdgeMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteEdge",
      requestType = com.astraeadb.grpc.proto.DeleteEdgeRequest.class,
      responseType = com.astraeadb.grpc.proto.MutationResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.DeleteEdgeRequest,
      com.astraeadb.grpc.proto.MutationResponse> getDeleteEdgeMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.DeleteEdgeRequest, com.astraeadb.grpc.proto.MutationResponse> getDeleteEdgeMethod;
    if ((getDeleteEdgeMethod = AstraeaServiceGrpc.getDeleteEdgeMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getDeleteEdgeMethod = AstraeaServiceGrpc.getDeleteEdgeMethod) == null) {
          AstraeaServiceGrpc.getDeleteEdgeMethod = getDeleteEdgeMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.DeleteEdgeRequest, com.astraeadb.grpc.proto.MutationResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteEdge"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.DeleteEdgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.MutationResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("DeleteEdge"))
              .build();
        }
      }
    }
    return getDeleteEdgeMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.NeighborsRequest,
      com.astraeadb.grpc.proto.NeighborsResponse> getNeighborsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Neighbors",
      requestType = com.astraeadb.grpc.proto.NeighborsRequest.class,
      responseType = com.astraeadb.grpc.proto.NeighborsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.NeighborsRequest,
      com.astraeadb.grpc.proto.NeighborsResponse> getNeighborsMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.NeighborsRequest, com.astraeadb.grpc.proto.NeighborsResponse> getNeighborsMethod;
    if ((getNeighborsMethod = AstraeaServiceGrpc.getNeighborsMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getNeighborsMethod = AstraeaServiceGrpc.getNeighborsMethod) == null) {
          AstraeaServiceGrpc.getNeighborsMethod = getNeighborsMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.NeighborsRequest, com.astraeadb.grpc.proto.NeighborsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Neighbors"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.NeighborsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.NeighborsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("Neighbors"))
              .build();
        }
      }
    }
    return getNeighborsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.BfsRequest,
      com.astraeadb.grpc.proto.BfsResponse> getBfsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Bfs",
      requestType = com.astraeadb.grpc.proto.BfsRequest.class,
      responseType = com.astraeadb.grpc.proto.BfsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.BfsRequest,
      com.astraeadb.grpc.proto.BfsResponse> getBfsMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.BfsRequest, com.astraeadb.grpc.proto.BfsResponse> getBfsMethod;
    if ((getBfsMethod = AstraeaServiceGrpc.getBfsMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getBfsMethod = AstraeaServiceGrpc.getBfsMethod) == null) {
          AstraeaServiceGrpc.getBfsMethod = getBfsMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.BfsRequest, com.astraeadb.grpc.proto.BfsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Bfs"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.BfsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.BfsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("Bfs"))
              .build();
        }
      }
    }
    return getBfsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.ShortestPathRequest,
      com.astraeadb.grpc.proto.ShortestPathResponse> getShortestPathMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ShortestPath",
      requestType = com.astraeadb.grpc.proto.ShortestPathRequest.class,
      responseType = com.astraeadb.grpc.proto.ShortestPathResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.ShortestPathRequest,
      com.astraeadb.grpc.proto.ShortestPathResponse> getShortestPathMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.ShortestPathRequest, com.astraeadb.grpc.proto.ShortestPathResponse> getShortestPathMethod;
    if ((getShortestPathMethod = AstraeaServiceGrpc.getShortestPathMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getShortestPathMethod = AstraeaServiceGrpc.getShortestPathMethod) == null) {
          AstraeaServiceGrpc.getShortestPathMethod = getShortestPathMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.ShortestPathRequest, com.astraeadb.grpc.proto.ShortestPathResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ShortestPath"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.ShortestPathRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.ShortestPathResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("ShortestPath"))
              .build();
        }
      }
    }
    return getShortestPathMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.VectorSearchRequest,
      com.astraeadb.grpc.proto.VectorSearchResponse> getVectorSearchMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "VectorSearch",
      requestType = com.astraeadb.grpc.proto.VectorSearchRequest.class,
      responseType = com.astraeadb.grpc.proto.VectorSearchResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.VectorSearchRequest,
      com.astraeadb.grpc.proto.VectorSearchResponse> getVectorSearchMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.VectorSearchRequest, com.astraeadb.grpc.proto.VectorSearchResponse> getVectorSearchMethod;
    if ((getVectorSearchMethod = AstraeaServiceGrpc.getVectorSearchMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getVectorSearchMethod = AstraeaServiceGrpc.getVectorSearchMethod) == null) {
          AstraeaServiceGrpc.getVectorSearchMethod = getVectorSearchMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.VectorSearchRequest, com.astraeadb.grpc.proto.VectorSearchResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "VectorSearch"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.VectorSearchRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.VectorSearchResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("VectorSearch"))
              .build();
        }
      }
    }
    return getVectorSearchMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.QueryRequest,
      com.astraeadb.grpc.proto.QueryResponse> getQueryMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Query",
      requestType = com.astraeadb.grpc.proto.QueryRequest.class,
      responseType = com.astraeadb.grpc.proto.QueryResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.QueryRequest,
      com.astraeadb.grpc.proto.QueryResponse> getQueryMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.QueryRequest, com.astraeadb.grpc.proto.QueryResponse> getQueryMethod;
    if ((getQueryMethod = AstraeaServiceGrpc.getQueryMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getQueryMethod = AstraeaServiceGrpc.getQueryMethod) == null) {
          AstraeaServiceGrpc.getQueryMethod = getQueryMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.QueryRequest, com.astraeadb.grpc.proto.QueryResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Query"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.QueryRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.QueryResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("Query"))
              .build();
        }
      }
    }
    return getQueryMethod;
  }

  private static volatile io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.PingRequest,
      com.astraeadb.grpc.proto.PingResponse> getPingMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Ping",
      requestType = com.astraeadb.grpc.proto.PingRequest.class,
      responseType = com.astraeadb.grpc.proto.PingResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.PingRequest,
      com.astraeadb.grpc.proto.PingResponse> getPingMethod() {
    io.grpc.MethodDescriptor<com.astraeadb.grpc.proto.PingRequest, com.astraeadb.grpc.proto.PingResponse> getPingMethod;
    if ((getPingMethod = AstraeaServiceGrpc.getPingMethod) == null) {
      synchronized (AstraeaServiceGrpc.class) {
        if ((getPingMethod = AstraeaServiceGrpc.getPingMethod) == null) {
          AstraeaServiceGrpc.getPingMethod = getPingMethod =
              io.grpc.MethodDescriptor.<com.astraeadb.grpc.proto.PingRequest, com.astraeadb.grpc.proto.PingResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Ping"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.PingRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  com.astraeadb.grpc.proto.PingResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AstraeaServiceMethodDescriptorSupplier("Ping"))
              .build();
        }
      }
    }
    return getPingMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static AstraeaServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceStub>() {
        @java.lang.Override
        public AstraeaServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AstraeaServiceStub(channel, callOptions);
        }
      };
    return AstraeaServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static AstraeaServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceBlockingV2Stub>() {
        @java.lang.Override
        public AstraeaServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AstraeaServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return AstraeaServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static AstraeaServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceBlockingStub>() {
        @java.lang.Override
        public AstraeaServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AstraeaServiceBlockingStub(channel, callOptions);
        }
      };
    return AstraeaServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static AstraeaServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AstraeaServiceFutureStub>() {
        @java.lang.Override
        public AstraeaServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AstraeaServiceFutureStub(channel, callOptions);
        }
      };
    return AstraeaServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     * <pre>
     * Node CRUD
     * </pre>
     */
    default void createNode(com.astraeadb.grpc.proto.CreateNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateNodeMethod(), responseObserver);
    }

    /**
     */
    default void getNode(com.astraeadb.grpc.proto.GetNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.GetNodeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetNodeMethod(), responseObserver);
    }

    /**
     */
    default void updateNode(com.astraeadb.grpc.proto.UpdateNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getUpdateNodeMethod(), responseObserver);
    }

    /**
     */
    default void deleteNode(com.astraeadb.grpc.proto.DeleteNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteNodeMethod(), responseObserver);
    }

    /**
     * <pre>
     * Edge CRUD
     * </pre>
     */
    default void createEdge(com.astraeadb.grpc.proto.CreateEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateEdgeMethod(), responseObserver);
    }

    /**
     */
    default void getEdge(com.astraeadb.grpc.proto.GetEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.GetEdgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetEdgeMethod(), responseObserver);
    }

    /**
     */
    default void updateEdge(com.astraeadb.grpc.proto.UpdateEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getUpdateEdgeMethod(), responseObserver);
    }

    /**
     */
    default void deleteEdge(com.astraeadb.grpc.proto.DeleteEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteEdgeMethod(), responseObserver);
    }

    /**
     * <pre>
     * Graph traversal
     * </pre>
     */
    default void neighbors(com.astraeadb.grpc.proto.NeighborsRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.NeighborsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getNeighborsMethod(), responseObserver);
    }

    /**
     */
    default void bfs(com.astraeadb.grpc.proto.BfsRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.BfsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getBfsMethod(), responseObserver);
    }

    /**
     */
    default void shortestPath(com.astraeadb.grpc.proto.ShortestPathRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.ShortestPathResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getShortestPathMethod(), responseObserver);
    }

    /**
     * <pre>
     * Vector search
     * </pre>
     */
    default void vectorSearch(com.astraeadb.grpc.proto.VectorSearchRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.VectorSearchResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getVectorSearchMethod(), responseObserver);
    }

    /**
     * <pre>
     * GQL query
     * </pre>
     */
    default void query(com.astraeadb.grpc.proto.QueryRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.QueryResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getQueryMethod(), responseObserver);
    }

    /**
     * <pre>
     * Health check
     * </pre>
     */
    default void ping(com.astraeadb.grpc.proto.PingRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.PingResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getPingMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service AstraeaService.
   */
  public static abstract class AstraeaServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return AstraeaServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service AstraeaService.
   */
  public static final class AstraeaServiceStub
      extends io.grpc.stub.AbstractAsyncStub<AstraeaServiceStub> {
    private AstraeaServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AstraeaServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AstraeaServiceStub(channel, callOptions);
    }

    /**
     * <pre>
     * Node CRUD
     * </pre>
     */
    public void createNode(com.astraeadb.grpc.proto.CreateNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateNodeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getNode(com.astraeadb.grpc.proto.GetNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.GetNodeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetNodeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void updateNode(com.astraeadb.grpc.proto.UpdateNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getUpdateNodeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteNode(com.astraeadb.grpc.proto.DeleteNodeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteNodeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Edge CRUD
     * </pre>
     */
    public void createEdge(com.astraeadb.grpc.proto.CreateEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateEdgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getEdge(com.astraeadb.grpc.proto.GetEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.GetEdgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetEdgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void updateEdge(com.astraeadb.grpc.proto.UpdateEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getUpdateEdgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteEdge(com.astraeadb.grpc.proto.DeleteEdgeRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteEdgeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Graph traversal
     * </pre>
     */
    public void neighbors(com.astraeadb.grpc.proto.NeighborsRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.NeighborsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getNeighborsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void bfs(com.astraeadb.grpc.proto.BfsRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.BfsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getBfsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void shortestPath(com.astraeadb.grpc.proto.ShortestPathRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.ShortestPathResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getShortestPathMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Vector search
     * </pre>
     */
    public void vectorSearch(com.astraeadb.grpc.proto.VectorSearchRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.VectorSearchResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getVectorSearchMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * GQL query
     * </pre>
     */
    public void query(com.astraeadb.grpc.proto.QueryRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.QueryResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getQueryMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * Health check
     * </pre>
     */
    public void ping(com.astraeadb.grpc.proto.PingRequest request,
        io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.PingResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getPingMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service AstraeaService.
   */
  public static final class AstraeaServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<AstraeaServiceBlockingV2Stub> {
    private AstraeaServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AstraeaServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AstraeaServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     * <pre>
     * Node CRUD
     * </pre>
     */
    public com.astraeadb.grpc.proto.MutationResponse createNode(com.astraeadb.grpc.proto.CreateNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.GetNodeResponse getNode(com.astraeadb.grpc.proto.GetNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse updateNode(com.astraeadb.grpc.proto.UpdateNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getUpdateNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse deleteNode(com.astraeadb.grpc.proto.DeleteNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteNodeMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Edge CRUD
     * </pre>
     */
    public com.astraeadb.grpc.proto.MutationResponse createEdge(com.astraeadb.grpc.proto.CreateEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateEdgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.GetEdgeResponse getEdge(com.astraeadb.grpc.proto.GetEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetEdgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse updateEdge(com.astraeadb.grpc.proto.UpdateEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getUpdateEdgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse deleteEdge(com.astraeadb.grpc.proto.DeleteEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteEdgeMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Graph traversal
     * </pre>
     */
    public com.astraeadb.grpc.proto.NeighborsResponse neighbors(com.astraeadb.grpc.proto.NeighborsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getNeighborsMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.BfsResponse bfs(com.astraeadb.grpc.proto.BfsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getBfsMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.ShortestPathResponse shortestPath(com.astraeadb.grpc.proto.ShortestPathRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getShortestPathMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Vector search
     * </pre>
     */
    public com.astraeadb.grpc.proto.VectorSearchResponse vectorSearch(com.astraeadb.grpc.proto.VectorSearchRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getVectorSearchMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * GQL query
     * </pre>
     */
    public com.astraeadb.grpc.proto.QueryResponse query(com.astraeadb.grpc.proto.QueryRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getQueryMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Health check
     * </pre>
     */
    public com.astraeadb.grpc.proto.PingResponse ping(com.astraeadb.grpc.proto.PingRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getPingMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service AstraeaService.
   */
  public static final class AstraeaServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<AstraeaServiceBlockingStub> {
    private AstraeaServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AstraeaServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AstraeaServiceBlockingStub(channel, callOptions);
    }

    /**
     * <pre>
     * Node CRUD
     * </pre>
     */
    public com.astraeadb.grpc.proto.MutationResponse createNode(com.astraeadb.grpc.proto.CreateNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.GetNodeResponse getNode(com.astraeadb.grpc.proto.GetNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse updateNode(com.astraeadb.grpc.proto.UpdateNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getUpdateNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse deleteNode(com.astraeadb.grpc.proto.DeleteNodeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteNodeMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Edge CRUD
     * </pre>
     */
    public com.astraeadb.grpc.proto.MutationResponse createEdge(com.astraeadb.grpc.proto.CreateEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateEdgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.GetEdgeResponse getEdge(com.astraeadb.grpc.proto.GetEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetEdgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse updateEdge(com.astraeadb.grpc.proto.UpdateEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getUpdateEdgeMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.MutationResponse deleteEdge(com.astraeadb.grpc.proto.DeleteEdgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteEdgeMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Graph traversal
     * </pre>
     */
    public com.astraeadb.grpc.proto.NeighborsResponse neighbors(com.astraeadb.grpc.proto.NeighborsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getNeighborsMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.BfsResponse bfs(com.astraeadb.grpc.proto.BfsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getBfsMethod(), getCallOptions(), request);
    }

    /**
     */
    public com.astraeadb.grpc.proto.ShortestPathResponse shortestPath(com.astraeadb.grpc.proto.ShortestPathRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getShortestPathMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Vector search
     * </pre>
     */
    public com.astraeadb.grpc.proto.VectorSearchResponse vectorSearch(com.astraeadb.grpc.proto.VectorSearchRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getVectorSearchMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * GQL query
     * </pre>
     */
    public com.astraeadb.grpc.proto.QueryResponse query(com.astraeadb.grpc.proto.QueryRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getQueryMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * Health check
     * </pre>
     */
    public com.astraeadb.grpc.proto.PingResponse ping(com.astraeadb.grpc.proto.PingRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getPingMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service AstraeaService.
   */
  public static final class AstraeaServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<AstraeaServiceFutureStub> {
    private AstraeaServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AstraeaServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AstraeaServiceFutureStub(channel, callOptions);
    }

    /**
     * <pre>
     * Node CRUD
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.MutationResponse> createNode(
        com.astraeadb.grpc.proto.CreateNodeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateNodeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.GetNodeResponse> getNode(
        com.astraeadb.grpc.proto.GetNodeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetNodeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.MutationResponse> updateNode(
        com.astraeadb.grpc.proto.UpdateNodeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getUpdateNodeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.MutationResponse> deleteNode(
        com.astraeadb.grpc.proto.DeleteNodeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteNodeMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Edge CRUD
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.MutationResponse> createEdge(
        com.astraeadb.grpc.proto.CreateEdgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateEdgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.GetEdgeResponse> getEdge(
        com.astraeadb.grpc.proto.GetEdgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetEdgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.MutationResponse> updateEdge(
        com.astraeadb.grpc.proto.UpdateEdgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getUpdateEdgeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.MutationResponse> deleteEdge(
        com.astraeadb.grpc.proto.DeleteEdgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteEdgeMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Graph traversal
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.NeighborsResponse> neighbors(
        com.astraeadb.grpc.proto.NeighborsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getNeighborsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.BfsResponse> bfs(
        com.astraeadb.grpc.proto.BfsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getBfsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.ShortestPathResponse> shortestPath(
        com.astraeadb.grpc.proto.ShortestPathRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getShortestPathMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Vector search
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.VectorSearchResponse> vectorSearch(
        com.astraeadb.grpc.proto.VectorSearchRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getVectorSearchMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * GQL query
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.QueryResponse> query(
        com.astraeadb.grpc.proto.QueryRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getQueryMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * Health check
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<com.astraeadb.grpc.proto.PingResponse> ping(
        com.astraeadb.grpc.proto.PingRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getPingMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE_NODE = 0;
  private static final int METHODID_GET_NODE = 1;
  private static final int METHODID_UPDATE_NODE = 2;
  private static final int METHODID_DELETE_NODE = 3;
  private static final int METHODID_CREATE_EDGE = 4;
  private static final int METHODID_GET_EDGE = 5;
  private static final int METHODID_UPDATE_EDGE = 6;
  private static final int METHODID_DELETE_EDGE = 7;
  private static final int METHODID_NEIGHBORS = 8;
  private static final int METHODID_BFS = 9;
  private static final int METHODID_SHORTEST_PATH = 10;
  private static final int METHODID_VECTOR_SEARCH = 11;
  private static final int METHODID_QUERY = 12;
  private static final int METHODID_PING = 13;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final AsyncService serviceImpl;
    private final int methodId;

    MethodHandlers(AsyncService serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_CREATE_NODE:
          serviceImpl.createNode((com.astraeadb.grpc.proto.CreateNodeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse>) responseObserver);
          break;
        case METHODID_GET_NODE:
          serviceImpl.getNode((com.astraeadb.grpc.proto.GetNodeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.GetNodeResponse>) responseObserver);
          break;
        case METHODID_UPDATE_NODE:
          serviceImpl.updateNode((com.astraeadb.grpc.proto.UpdateNodeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse>) responseObserver);
          break;
        case METHODID_DELETE_NODE:
          serviceImpl.deleteNode((com.astraeadb.grpc.proto.DeleteNodeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse>) responseObserver);
          break;
        case METHODID_CREATE_EDGE:
          serviceImpl.createEdge((com.astraeadb.grpc.proto.CreateEdgeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse>) responseObserver);
          break;
        case METHODID_GET_EDGE:
          serviceImpl.getEdge((com.astraeadb.grpc.proto.GetEdgeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.GetEdgeResponse>) responseObserver);
          break;
        case METHODID_UPDATE_EDGE:
          serviceImpl.updateEdge((com.astraeadb.grpc.proto.UpdateEdgeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse>) responseObserver);
          break;
        case METHODID_DELETE_EDGE:
          serviceImpl.deleteEdge((com.astraeadb.grpc.proto.DeleteEdgeRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.MutationResponse>) responseObserver);
          break;
        case METHODID_NEIGHBORS:
          serviceImpl.neighbors((com.astraeadb.grpc.proto.NeighborsRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.NeighborsResponse>) responseObserver);
          break;
        case METHODID_BFS:
          serviceImpl.bfs((com.astraeadb.grpc.proto.BfsRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.BfsResponse>) responseObserver);
          break;
        case METHODID_SHORTEST_PATH:
          serviceImpl.shortestPath((com.astraeadb.grpc.proto.ShortestPathRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.ShortestPathResponse>) responseObserver);
          break;
        case METHODID_VECTOR_SEARCH:
          serviceImpl.vectorSearch((com.astraeadb.grpc.proto.VectorSearchRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.VectorSearchResponse>) responseObserver);
          break;
        case METHODID_QUERY:
          serviceImpl.query((com.astraeadb.grpc.proto.QueryRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.QueryResponse>) responseObserver);
          break;
        case METHODID_PING:
          serviceImpl.ping((com.astraeadb.grpc.proto.PingRequest) request,
              (io.grpc.stub.StreamObserver<com.astraeadb.grpc.proto.PingResponse>) responseObserver);
          break;
        default:
          throw new AssertionError();
      }
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public io.grpc.stub.StreamObserver<Req> invoke(
        io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        default:
          throw new AssertionError();
      }
    }
  }

  public static final io.grpc.ServerServiceDefinition bindService(AsyncService service) {
    return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
        .addMethod(
          getCreateNodeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.CreateNodeRequest,
              com.astraeadb.grpc.proto.MutationResponse>(
                service, METHODID_CREATE_NODE)))
        .addMethod(
          getGetNodeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.GetNodeRequest,
              com.astraeadb.grpc.proto.GetNodeResponse>(
                service, METHODID_GET_NODE)))
        .addMethod(
          getUpdateNodeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.UpdateNodeRequest,
              com.astraeadb.grpc.proto.MutationResponse>(
                service, METHODID_UPDATE_NODE)))
        .addMethod(
          getDeleteNodeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.DeleteNodeRequest,
              com.astraeadb.grpc.proto.MutationResponse>(
                service, METHODID_DELETE_NODE)))
        .addMethod(
          getCreateEdgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.CreateEdgeRequest,
              com.astraeadb.grpc.proto.MutationResponse>(
                service, METHODID_CREATE_EDGE)))
        .addMethod(
          getGetEdgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.GetEdgeRequest,
              com.astraeadb.grpc.proto.GetEdgeResponse>(
                service, METHODID_GET_EDGE)))
        .addMethod(
          getUpdateEdgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.UpdateEdgeRequest,
              com.astraeadb.grpc.proto.MutationResponse>(
                service, METHODID_UPDATE_EDGE)))
        .addMethod(
          getDeleteEdgeMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.DeleteEdgeRequest,
              com.astraeadb.grpc.proto.MutationResponse>(
                service, METHODID_DELETE_EDGE)))
        .addMethod(
          getNeighborsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.NeighborsRequest,
              com.astraeadb.grpc.proto.NeighborsResponse>(
                service, METHODID_NEIGHBORS)))
        .addMethod(
          getBfsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.BfsRequest,
              com.astraeadb.grpc.proto.BfsResponse>(
                service, METHODID_BFS)))
        .addMethod(
          getShortestPathMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.ShortestPathRequest,
              com.astraeadb.grpc.proto.ShortestPathResponse>(
                service, METHODID_SHORTEST_PATH)))
        .addMethod(
          getVectorSearchMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.VectorSearchRequest,
              com.astraeadb.grpc.proto.VectorSearchResponse>(
                service, METHODID_VECTOR_SEARCH)))
        .addMethod(
          getQueryMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.QueryRequest,
              com.astraeadb.grpc.proto.QueryResponse>(
                service, METHODID_QUERY)))
        .addMethod(
          getPingMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              com.astraeadb.grpc.proto.PingRequest,
              com.astraeadb.grpc.proto.PingResponse>(
                service, METHODID_PING)))
        .build();
  }

  private static abstract class AstraeaServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    AstraeaServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return com.astraeadb.grpc.proto.AstraeaProto.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("AstraeaService");
    }
  }

  private static final class AstraeaServiceFileDescriptorSupplier
      extends AstraeaServiceBaseDescriptorSupplier {
    AstraeaServiceFileDescriptorSupplier() {}
  }

  private static final class AstraeaServiceMethodDescriptorSupplier
      extends AstraeaServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    AstraeaServiceMethodDescriptorSupplier(java.lang.String methodName) {
      this.methodName = methodName;
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.MethodDescriptor getMethodDescriptor() {
      return getServiceDescriptor().findMethodByName(methodName);
    }
  }

  private static volatile io.grpc.ServiceDescriptor serviceDescriptor;

  public static io.grpc.ServiceDescriptor getServiceDescriptor() {
    io.grpc.ServiceDescriptor result = serviceDescriptor;
    if (result == null) {
      synchronized (AstraeaServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new AstraeaServiceFileDescriptorSupplier())
              .addMethod(getCreateNodeMethod())
              .addMethod(getGetNodeMethod())
              .addMethod(getUpdateNodeMethod())
              .addMethod(getDeleteNodeMethod())
              .addMethod(getCreateEdgeMethod())
              .addMethod(getGetEdgeMethod())
              .addMethod(getUpdateEdgeMethod())
              .addMethod(getDeleteEdgeMethod())
              .addMethod(getNeighborsMethod())
              .addMethod(getBfsMethod())
              .addMethod(getShortestPathMethod())
              .addMethod(getVectorSearchMethod())
              .addMethod(getQueryMethod())
              .addMethod(getPingMethod())
              .build();
        }
      }
    }
    return result;
  }
}
