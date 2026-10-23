const ARTIFACTS = [
    project.schema.json
    model.schema.json
]

def main []: nothing -> nothing {
    rojo-schema generate

    let files = $ARTIFACTS | each {|name|
        let path = [dist $name] | path join

        if not ($path | path exists) {
            error make $"missing artifact: ($path)"
        }

        $path
    }

    if ("site" | path exists) {
        rm --recursive site
    }

    mkdir site/latest

    for file in $files {
        cp $file site/latest
    }
}

def "main resolve" []: nothing -> nothing {
    let releases = gh release list --exclude-drafts --exclude-pre-releases --limit 1 --json tagName | from json

    if ($releases | is-empty) {
        "No published stable release; skipping Pages.\n" | save --append $env.GITHUB_STEP_SUMMARY

        return
    }

    let latest = gh release view --json tagName | from json | get tagName

    if ($env.REQUESTED_RELEASE | is-not-empty) and $env.REQUESTED_RELEASE != $latest {
        "Requested release is not the latest stable release; skipping Pages.\n" | save --append $env.GITHUB_STEP_SUMMARY

        return
    }

    $"tag=($latest)\n" | save --append $env.GITHUB_OUTPUT
}
