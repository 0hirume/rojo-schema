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

def "main stage" []: nothing -> nothing {
    try {
        let files = (glob "dist/*")

        if ($files | is-empty) {
            fail "dist is empty"
        }

        if ("site" | path exists) {
            rm --recursive site
        }

        mkdir site/latest

        for file in $files {
            cp --recursive $file site/latest
        }
    } catch {|error| fail $error.msg }
}
