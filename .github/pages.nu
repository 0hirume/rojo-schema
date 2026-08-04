const ARTIFACTS = [
    project.schema.json
    model.schema.json
    manifest.json
    coverage.json
]

def fail [message: string]: nothing -> error {
    error make {
        msg: $message
        label: {
            text: $message
            span: (metadata $message).span
        }
    }
}

def checked [program: string, ...arguments: string]: nothing -> nothing {
    run-external $program ...$arguments

    let exit_code = $env.LAST_EXIT_CODE

    if $exit_code != 0 {
        fail $"($program) failed with exit code ($exit_code)"
    }
}

def main []: nothing -> error {
    fail "subcommand required"
}

def artifacts []: nothing -> list<string> {
    $ARTIFACTS | each {|name|
        let path = [dist $name] | path join

        if not ($path | path exists) {
            fail $"missing artifact: ($path)"
        }

        $path
    }
}

def store-path []: nothing -> string {
    [$env.RUNNER_TEMP $"rojo-schema-snapshots-($env.GITHUB_RUN_ID)"] | path join
}

def clone-store [store: string]: nothing -> nothing {
    checked gh auth setup-git

    let remote = $"https://github.com/($env.REPOSITORY).git"
    let branch = (
        run-external git ls-remote "--exit-code" "--heads" $remote $env.SNAPSHOT_BRANCH
        | complete
    )

    if $branch.exit_code == 0 {
        (checked
            gh
            repo
            clone
            $env.REPOSITORY
            $store
            "--"
            "--branch"
            $env.SNAPSHOT_BRANCH
            "--depth"
            "1"
            "--single-branch"
        )
    } else if $branch.exit_code == 2 {
        checked git init "--initial-branch" $env.SNAPSHOT_BRANCH $store
        checked git "-C" $store remote add origin $remote
    } else {
        fail $"checking snapshot branch failed with exit code ($branch.exit_code): ($branch.stderr | str trim)"
    }
}

def "main clone-sources" []: nothing -> nothing {
    checked gh repo clone $env.ROJO_REPOSITORY sources/rojo "--" "--depth" "1"
    (checked
        gh
        repo
        clone
        $env.DOCS_REPOSITORY
        sources/creator-docs
        "--"
        "--depth"
        "1"
        "--filter=blob:none"
        "--sparse"
    )
    (checked
        git
        "-C"
        sources/creator-docs
        sparse-checkout
        set
        content/en-us/reference/engine
    )
}

def "main snapshot" []: nothing -> nothing {
    try {
        let store = store-path
        clone-store $store

        let hash = (
            artifacts
            | each {|path| open --raw $path }
            | str join (char nul)
            | hash sha256
        )
        let index_path = [$store index.json] | path join
        let index = if ($index_path | path exists) {
            open $index_path
        } else {
            {
                latest: null
                snapshots: []
            }
        }
        let previous = $index | get --optional latest.sha256

        if $previous == $hash {
            print $"schema unchanged: ($hash)"
        } else {
            let now = date now | date to-timezone "+0000"
            let created_at = $now | format date %Y-%m-%dT%H:%M:%SZ
            let stamp = $now | format date %Y-%m-%d-%H%M%SZ
            let short = $hash | str substring 0..<12
            let id = $"($stamp)-($short)"
            let destination = [$store $id] | path join
            mkdir $destination

            for artifact in (artifacts) {
                cp $artifact $destination
            }

            let entry = (
                {}
                | insert id $id
                | insert createdAt $created_at
                | insert sha256 $hash
                | insert artifacts $ARTIFACTS
            )
            let snapshots = $index | get --optional snapshots | default []
            {
                latest: $entry
                snapshots: ($snapshots | prepend $entry)
            } | to json --indent 2 | save --force $index_path

            checked git "-C" $store config user.name "github-actions[bot]"
            (checked
                git
                "-C"
                $store
                config
                user.email
                "41898282+github-actions[bot]@users.noreply.github.com"
            )
            checked git "-C" $store add "--all"
            checked git "-C" $store commit "-m" $"snapshot: ($id)"
            (checked
                git
                "-C"
                $store
                push
                "--set-upstream"
                origin
                $env.SNAPSHOT_BRANCH
            )
        }
    } catch {|error| fail $error.msg }
}

def "main stage" []: nothing -> nothing {
    try {
        let files = artifacts
        let store = store-path
        let index = [$store index.json] | path join

        if not ($index | path exists) {
            fail "snapshot index is missing"
        }

        if ("site" | path exists) {
            rm --recursive site
        }

        mkdir site/latest
        mkdir site/snapshots

        for file in $files {
            cp $file site/latest
        }

        for file in (ls $store | get name) {
            cp --recursive $file site/snapshots
        }

        cp .github/pages.html site/index.html
    } catch {|error| fail $error.msg }
}
