package com.astraeadb.grpc;

import com.astraeadb.grpc.proto.*;
import com.google.protobuf.DoubleValue;
import com.google.protobuf.Int64Value;
import io.grpc.stub.StreamObserver;

/**
 * Mock implementation of the AstraeaDB gRPC service for in-process testing.
 * Returns canned responses for all 14 supported RPCs.
 */
class MockAstraeaService extends AstraeaServiceGrpc.AstraeaServiceImplBase {

    // -----------------------------------------------------------------------
    // Health
    // -----------------------------------------------------------------------

    @Override
    public void ping(PingRequest req, StreamObserver<PingResponse> observer) {
        observer.onNext(PingResponse.newBuilder()
                .setPong(true)
                .setVersion("1.0.0-test")
                .build());
        observer.onCompleted();
    }

    // -----------------------------------------------------------------------
    // Node CRUD
    // -----------------------------------------------------------------------

    @Override
    public void createNode(CreateNodeRequest req, StreamObserver<MutationResponse> observer) {
        observer.onNext(MutationResponse.newBuilder()
                .setSuccess(true)
                .setResultJson("{\"node_id\":42}")
                .build());
        observer.onCompleted();
    }

    @Override
    public void getNode(GetNodeRequest req, StreamObserver<GetNodeResponse> observer) {
        if (req.getId() == 999) {
            observer.onNext(GetNodeResponse.newBuilder()
                    .setFound(false)
                    .setError("node not found with id 999")
                    .build());
        } else {
            observer.onNext(GetNodeResponse.newBuilder()
                    .setFound(true)
                    .setId(req.getId())
                    .addLabels("Person")
                    .setPropertiesJson("{\"name\":\"Alice\"}")
                    .setHasEmbedding(false)
                    .build());
        }
        observer.onCompleted();
    }

    @Override
    public void updateNode(UpdateNodeRequest req, StreamObserver<MutationResponse> observer) {
        if (req.getId() == 999) {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(false)
                    .setError("node not found with id 999")
                    .build());
        } else {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(true)
                    .setResultJson("{\"updated\":true}")
                    .build());
        }
        observer.onCompleted();
    }

    @Override
    public void deleteNode(DeleteNodeRequest req, StreamObserver<MutationResponse> observer) {
        if (req.getId() == 999) {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(false)
                    .setError("node not found with id 999")
                    .build());
        } else {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(true)
                    .setResultJson("{\"deleted\":true}")
                    .build());
        }
        observer.onCompleted();
    }

    // -----------------------------------------------------------------------
    // Edge CRUD
    // -----------------------------------------------------------------------

    @Override
    public void createEdge(CreateEdgeRequest req, StreamObserver<MutationResponse> observer) {
        observer.onNext(MutationResponse.newBuilder()
                .setSuccess(true)
                .setResultJson("{\"edge_id\":100}")
                .build());
        observer.onCompleted();
    }

    @Override
    public void getEdge(GetEdgeRequest req, StreamObserver<GetEdgeResponse> observer) {
        if (req.getId() == 999) {
            observer.onNext(GetEdgeResponse.newBuilder()
                    .setFound(false)
                    .setError("edge not found with id 999")
                    .build());
        } else {
            observer.onNext(GetEdgeResponse.newBuilder()
                    .setFound(true)
                    .setId(req.getId())
                    .setSource(1)
                    .setTarget(2)
                    .setEdgeType("KNOWS")
                    .setPropertiesJson("{\"since\":2020}")
                    .setWeight(1.5)
                    .setValidFrom(Int64Value.of(1000L))
                    .setValidTo(Int64Value.of(2000L))
                    .build());
        }
        observer.onCompleted();
    }

    @Override
    public void updateEdge(UpdateEdgeRequest req, StreamObserver<MutationResponse> observer) {
        if (req.getId() == 999) {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(false)
                    .setError("edge not found with id 999")
                    .build());
        } else {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(true)
                    .setResultJson("{\"updated\":true}")
                    .build());
        }
        observer.onCompleted();
    }

    @Override
    public void deleteEdge(DeleteEdgeRequest req, StreamObserver<MutationResponse> observer) {
        if (req.getId() == 999) {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(false)
                    .setError("edge not found with id 999")
                    .build());
        } else {
            observer.onNext(MutationResponse.newBuilder()
                    .setSuccess(true)
                    .setResultJson("{\"deleted\":true}")
                    .build());
        }
        observer.onCompleted();
    }

    // -----------------------------------------------------------------------
    // Traversal
    // -----------------------------------------------------------------------

    @Override
    public void neighbors(NeighborsRequest req, StreamObserver<NeighborsResponse> observer) {
        observer.onNext(NeighborsResponse.newBuilder()
                .addNeighbors(NeighborEntry.newBuilder().setEdgeId(10).setNodeId(20).build())
                .addNeighbors(NeighborEntry.newBuilder().setEdgeId(11).setNodeId(21).build())
                .build());
        observer.onCompleted();
    }

    @Override
    public void bfs(BfsRequest req, StreamObserver<BfsResponse> observer) {
        observer.onNext(BfsResponse.newBuilder()
                .addNodes(BfsEntry.newBuilder().setNodeId(req.getStart()).setDepth(0).build())
                .addNodes(BfsEntry.newBuilder().setNodeId(20).setDepth(1).build())
                .addNodes(BfsEntry.newBuilder().setNodeId(30).setDepth(2).build())
                .build());
        observer.onCompleted();
    }

    @Override
    public void shortestPath(ShortestPathRequest req, StreamObserver<ShortestPathResponse> observer) {
        observer.onNext(ShortestPathResponse.newBuilder()
                .setFound(true)
                .addPath(req.getFrom())
                .addPath(5)
                .addPath(req.getTo())
                .setLength(2)
                .setCost(DoubleValue.of(3.5))
                .build());
        observer.onCompleted();
    }

    // -----------------------------------------------------------------------
    // Vector search
    // -----------------------------------------------------------------------

    @Override
    public void vectorSearch(VectorSearchRequest req, StreamObserver<VectorSearchResponse> observer) {
        var builder = VectorSearchResponse.newBuilder();
        int k = req.getK() > 0 ? req.getK() : 10;
        for (int i = 0; i < Math.min(k, 3); i++) {
            builder.addResults(VectorSearchResult.newBuilder()
                    .setNodeId(100 + i)
                    .setScore(0.95f - (i * 0.1f))
                    .build());
        }
        observer.onNext(builder.build());
        observer.onCompleted();
    }

    // -----------------------------------------------------------------------
    // GQL query
    // -----------------------------------------------------------------------

    @Override
    public void query(QueryRequest req, StreamObserver<QueryResponse> observer) {
        observer.onNext(QueryResponse.newBuilder()
                .setSuccess(true)
                .setResultJson("{\"columns\":[\"n\"],\"rows\":[[{\"id\":1}]],\"stats\":{\"nodes_created\":0,\"edges_created\":0,\"nodes_deleted\":0,\"edges_deleted\":0}}")
                .build());
        observer.onCompleted();
    }
}
