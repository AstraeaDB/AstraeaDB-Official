package com.astraeadb.exception;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;

import static org.assertj.core.api.Assertions.assertThat;

class ErrorClassifierTest {

    @Test
    void classifiesNodeNotFound() {
        var ex = ErrorClassifier.classify("node not found with id 42");
        assertThat(ex).isInstanceOf(NodeNotFoundException.class);
        assertThat(ex.getMessage()).contains("42");
    }

    @Test
    void classifiesEdgeNotFound() {
        var ex = ErrorClassifier.classify("edge not found");
        assertThat(ex).isInstanceOf(EdgeNotFoundException.class);
    }

    @Test
    void classifiesVectorIndexNotConfigured() {
        var ex = ErrorClassifier.classify("vector index not configured");
        assertThat(ex).isInstanceOf(VectorIndexNotConfiguredException.class);
    }

    @Test
    void classifiesAccessDenied() {
        var ex = ErrorClassifier.classify("access denied for operation");
        assertThat(ex).isInstanceOf(AccessDeniedException.class);
    }

    @Test
    void classifiesInvalidCredentials() {
        var ex = ErrorClassifier.classify("invalid credentials provided");
        assertThat(ex).isInstanceOf(InvalidCredentialsException.class);
    }

    @Test
    void classifiesAuthRequired() {
        var ex = ErrorClassifier.classify("authentication required");
        assertThat(ex).isInstanceOf(AuthRequiredException.class);
    }

    @Test
    void unknownErrorReturnsBaseException() {
        var ex = ErrorClassifier.classify("something went wrong");
        assertThat(ex).isExactlyInstanceOf(AstraeaException.class);
        assertThat(ex.getMessage()).isEqualTo("something went wrong");
    }

    @Test
    void nullMessageHandled() {
        var ex = ErrorClassifier.classify(null);
        assertThat(ex).isExactlyInstanceOf(AstraeaException.class);
    }
}
