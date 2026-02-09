dependencies {
    api(project(":astraeadb-api"))
    implementation(project(":astraeadb-json"))
    implementation(project(":astraeadb-grpc"))
    implementation(project(":astraeadb-flight"))

    testImplementation("org.junit.jupiter:junit-jupiter:${property("junitVersion")}")
    testImplementation("org.assertj:assertj-core:${property("assertjVersion")}")
}
