dependencies {
    api(project(":astraeadb-api"))
    implementation("com.fasterxml.jackson.core:jackson-databind:${property("jacksonVersion")}")
    implementation("org.apache.arrow:flight-core:${property("arrowVersion")}")
    implementation("org.apache.arrow:arrow-vector:${property("arrowVersion")}")
    implementation("org.apache.arrow:arrow-memory-netty:${property("arrowVersion")}")

    testImplementation("org.junit.jupiter:junit-jupiter:${property("junitVersion")}")
    testImplementation("org.assertj:assertj-core:${property("assertjVersion")}")
    testImplementation("org.apache.arrow:flight-core:${property("arrowVersion")}")
}
