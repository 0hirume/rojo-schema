def fail [message: string]: nothing -> error {
    error make {
        msg: $message
        label: {
            text: $message
            span: (metadata $message).span
        }
    }
}

def completed [program: string, ...arguments: string]: nothing -> record {
    run-external $program ...$arguments | complete
}

def checked [program: string, ...arguments: string]: nothing -> nothing {
    run-external $program ...$arguments

    let exit_code = $env.LAST_EXIT_CODE

    if $exit_code != 0 {
        fail $"($program) failed with exit code ($exit_code)"
    }
}

def captured [program: string, ...arguments: string]: nothing -> string {
    let result = (completed $program ...$arguments)

    if $result.exit_code != 0 {
        fail ($result.stderr | str trim)
    }

    $result.stdout | str trim
}

def "main clone-sources" []: nothing -> nothing {
    checked gh repo clone $env.ROJO_REPOSITORY sources/rojo "--" "--depth" "1"
    with-env {GIT_LFS_SKIP_SMUDGE: "1"} {
        checked gh repo clone $env.DOCS_REPOSITORY sources/creator-docs "--" "--depth" "1" "--filter=blob:none" "--sparse"
    }
    (checked
        git
        "-C"
        sources/creator-docs
        sparse-checkout
        set
        content/en-us/reference/engine
    )
}

def "main detect-changes" []: nothing -> nothing {
    let result = (completed git diff "--quiet" "--" dist)
    let changed = if $result.exit_code == 0 {
        false
    } else if $result.exit_code == 1 {
        true
    } else {
        fail ($result.stderr | str trim)
    }

    try {
        ($"changed=($changed)" + (char newline)) | save --append $env.GITHUB_OUTPUT
    } catch {|error| fail $error.msg }
}

def "main open-pull-request" []: nothing -> nothing {
    checked gh auth setup-git
    checked git config user.name "github-actions[bot]"
    (checked
        git
        config
        user.email
        "41898282+github-actions[bot]@users.noreply.github.com"
    )
    checked git switch "-c" $env.UPDATE_BRANCH
    checked git add dist
    checked git commit "-m" "chore: update schemas"

    let remote = (completed git ls-remote "--exit-code" "--heads" origin $env.UPDATE_BRANCH)

    if $remote.exit_code == 0 {
        (checked
            git
            fetch
            origin
            $"($env.UPDATE_BRANCH):refs/remotes/origin/($env.UPDATE_BRANCH)"
        )
    } else if $remote.exit_code != 2 {
        fail ($remote.stderr | str trim)
    }

    (checked
        git
        push
        "--force-with-lease"
        origin
        $"HEAD:refs/heads/($env.UPDATE_BRANCH)"
    )

    let pulls = (
        (captured
            gh
            pr
            list
            "--repo"
            $env.REPOSITORY
            "--head"
            $env.UPDATE_BRANCH
            "--base"
            $env.DEFAULT_BRANCH
            "--state"
            open
            "--json"
            number
            "--jq"
            length
        )
    )

    if $pulls == "0" {
        (checked
            gh
            pr
            create
            "--repo"
            $env.REPOSITORY
            "--head"
            $env.UPDATE_BRANCH
            "--base"
            $env.DEFAULT_BRANCH
            "--title"
            "chore: update schemas"
            "--body"
            "Regenerates the schema artifacts from the latest Rojo and Creator Docs default branches."
        )
    }
}
