plugins {
    id("com.google.protobuf") version "0.9.4"
}

dependencies {
    api(project(":astraeadb-api"))
    implementation("com.fasterxml.jackson.core:jackson-databind:${property("jacksonVersion")}")
    implementation("io.grpc:grpc-netty-shaded:${property("grpcVersion")}")
    implementation("io.grpc:grpc-protobuf:${property("grpcVersion")}")
    implementation("io.grpc:grpc-stub:${property("grpcVersion")}")
    implementation("com.google.protobuf:protobuf-java:${property("protocVersion")}")
    // Required for javax.annotation.Generated
    compileOnly("org.apache.tomcat:annotations-api:6.0.53")

    testImplementation("org.junit.jupiter:junit-jupiter:${property("junitVersion")}")
    testImplementation("org.assertj:assertj-core:${property("assertjVersion")}")
    testImplementation("io.grpc:grpc-testing:${property("grpcTestingVersion")}")
    testImplementation("io.grpc:grpc-inprocess:${property("grpcVersion")}")
}

protobuf {
    protoc {
        artifact = "com.google.protobuf:protoc:${property("protocVersion")}"
    }
    plugins {
        create("grpc") {
            artifact = "io.grpc:protoc-gen-grpc-java:${property("grpcVersion")}"
        }
    }
    generateProtoTasks {
        all().forEach { task ->
            task.plugins {
                create("grpc")
            }
        }
    }
}
