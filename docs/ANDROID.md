# Android integration

Soika accepts Java/Kotlin errors from the Sentry Android SDK and deobfuscates
ProGuard/R8 stack traces. Use your Soika project DSN for events and a separate
session token for build-time mapping uploads.

## 1. Connect the application

Create a team and project in Soika, then copy the DSN from the project settings.
The device must be able to reach that HTTPS address.

Add the Sentry plugin to the application's `build.gradle.kts` (alongside the
existing Android/Kotlin plugins) and the SDK dependency. Replace the version
placeholders with pinned versions compatible with your Android Gradle Plugin;
see Sentry's [Android setup](https://docs.sentry.io/platforms/android/manual-setup/)
and [Gradle plugin configuration](https://docs.sentry.io/platforms/android/configuration/gradle/).

```kotlin
plugins {
    id("io.sentry.android.gradle") version "<SENTRY_PLUGIN_VERSION>"
}

dependencies {
    implementation("io.sentry:sentry-android:<SENTRY_SDK_VERSION>")
}
```

In `app/src/main/AndroidManifest.xml`, add the Internet permission and SDK
configuration to the existing manifest:

```xml
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    <application>
        <meta-data android:name="io.sentry.dsn"
            android:value="https://PUBLIC_KEY@soika.example.com/PROJECT_ID" />
        <meta-data android:name="io.sentry.traces.sample-rate" android:value="0.0" />
        <meta-data android:name="io.sentry.ndk.enable" android:value="false" />
    </application>
</manifest>
```

Use the exact DSN copied from Soika. SDK auto-initialization captures unhandled
Java/Kotlin exceptions. To report a handled exception:

```kotlin
import io.sentry.Sentry

Sentry.captureException(IllegalStateException("Soika Android integration test"))
```

## 2. Upload release mappings

In Soika, sign in as an Admin of the project's team or an instance Owner/Manager.
In browser developer tools, open the site's cookies and copy the **value** of
`soika_session`. Store it as the CI secret `SENTRY_AUTH_TOKEN`, without the
`soika_session=` prefix or cookie attributes. It expires with the session;
logout/revocation invalidates it. Sentry-issued tokens do not work with Soika.
Never put this token in the APK, manifest, or committed files.

Add this to the application's `build.gradle.kts`:

```kotlin
sentry {
    url.set("https://soika.example.com/")
    org.set("<TEAM_SHORT_ID>")
    projectName.set("<PROJECT_SLUG>")
    authToken.set(System.getenv("SENTRY_AUTH_TOKEN"))
    includeProguardMapping.set(true)
    autoUploadProguardMapping.set(true)
    uploadNativeSymbols.set(false)
    includeSourceContext.set(false)
}
```

`TEAM_SHORT_ID` is the owning team's ID from its Soika URL;
`PROJECT_SLUG` is the project's `slug` (available in `GET /api/projects` when
signed in). Set Soika's deployment `BASE_URL` to its externally reachable URL,
which is also used for chunk uploads.

Build an R8-enabled release using your existing release configuration:

```sh
./gradlew :app:assembleRelease
```

The release build must have `isMinifyEnabled = true` to produce a mapping.
The plugin generates and packages the ProGuard UUID and uploads the matching
mapping. Check that the upload succeeds before distributing the APK/AAB.
Upload each build's own mapping, including each flavor's mapping.

For a separate upload step, configure the CLI with the same destination and
upload the exact mapping used by the application:

```sh
export SENTRY_URL=https://soika.example.com/
export SENTRY_ORG="<TEAM_SHORT_ID>"
export SENTRY_PROJECT="<PROJECT_SLUG>"
# Supply SENTRY_AUTH_TOKEN through the build environment.
sentry-cli upload-proguard app/build/outputs/mapping/release/mapping.txt
```

Keep the Gradle plugin's UUID metadata in that build. The UUID in the APK and
uploaded mapping must match.

## 3. Verify

Install the release build, trigger the handled test exception above, and open
its event in Soika. Confirm that the stack shows original class/method names
and line numbers. If it remains obfuscated, check that the event contains
`debug_meta.images` with `type: "proguard"` and the uploaded mapping's `uuid`.

- `401` on upload: renew the session token.
- `403`: check the uploader's team/instance role.
- `404`: check the team short ID, project slug, and owning team.
- Missing events: check the DSN and device connectivity to Soika.

Mappings are matched by project and UUID before grouping. Uploading a mapping
does not reprocess older events. ZIP and SHA-1 chunk uploads are supported;
the upload limit is 32 MiB. Soika supports error events, not Sentry performance,
replay, source bundles, or native NDK symbolication. See
[Operations](OPERATIONS.md#android-proguard--r8-mappings) for storage details.
