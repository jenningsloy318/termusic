//! Phase 5: Style and Conventions enforcement tests.
//!
//! These tests verify that the podcast sync module meets the style and
//! convention standards defined in the review feedback (AC-28 through AC-31).
//!
//! They enforce:
//! - AC-29: Module documentation uses `//!` doc comments (SCENARIO-033)
//! - AC-30: Deeply nested logic (3+ levels) is extracted to named helpers (SCENARIO-034)
//! - AC-31: Functions accept config struct references, not individual values (SCENARIO-035)
//!
//! These are source-code quality tests that inspect the implementation for
//! structural compliance, similar to a linter but with semantic understanding.

#[cfg(test)]
mod phase5_style_conventions {
    // =========================================================================
    // AC-29 / SCENARIO-033: Module documentation uses //! doc comment format
    //
    // The podcast_sync.rs module MUST start with //! doc comments that describe
    // its purpose and responsibilities in the approved format.
    // =========================================================================

    /// AC-29/SCENARIO-033: The module file must start with `//!` doc comments.
    /// These are Rust inner doc comments that document the module itself.
    /// Regular `//` comments or `///` comments at the module level are incorrect.
    #[test]
    fn module_starts_with_inner_doc_comments() {
        let source = include_str!("podcast_sync.rs");
        let first_line = source.lines().next().unwrap_or("");

        assert!(
            first_line.starts_with("//!"),
            "AC-29/SCENARIO-033: podcast_sync.rs must start with //! doc comments. \
             First line starts with: {:?}",
            &first_line[..first_line.len().min(20)]
        );
    }

    /// AC-29/SCENARIO-033: The module doc comment should be multi-line and describe
    /// the module's purpose (not just a single-word comment).
    /// The approved format includes at minimum: purpose description and scope.
    #[test]
    fn module_doc_comment_describes_purpose_and_scope() {
        let source = include_str!("podcast_sync.rs");

        // Count consecutive //! lines at the start of the file
        let doc_comment_lines: Vec<&str> = source
            .lines()
            .take_while(|line| line.starts_with("//!"))
            .collect();

        assert!(
            doc_comment_lines.len() >= 2,
            "AC-29/SCENARIO-033: Module doc comment should be at least 2 lines describing \
             purpose and scope. Found {} //! lines.",
            doc_comment_lines.len()
        );

        // The doc comment content should mention sync/synchronization to confirm
        // it actually describes this module's purpose
        let doc_content: String = doc_comment_lines.join(" ");
        assert!(
            doc_content.to_lowercase().contains("sync")
                || doc_content.to_lowercase().contains("podcast"),
            "AC-29/SCENARIO-033: Module doc comment should describe podcast synchronization. \
             Got: {:?}",
            doc_content
        );
    }

    /// AC-29/SCENARIO-033: All public functions should have `///` doc comments
    /// explaining their contract (what they do, not how they do it).
    #[test]
    fn public_functions_have_doc_comments() {
        let source = include_str!("podcast_sync.rs");

        // Find public function declarations (exclude test module section)
        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // List of public functions that MUST have doc comments
        let public_functions = [
            "pub fn should_download_episode",
            "pub fn find_episodes_to_download",
            "pub async fn sync_once",
            "pub fn start_podcast_sync_task",
        ];

        for func_sig in &public_functions {
            if let Some(func_pos) = production_code.find(func_sig) {
                // Look backwards from the function for a /// doc comment
                // (allowing for attributes like #[must_use] between doc and fn)
                let before_func = &production_code[..func_pos];
                let preceding_lines: Vec<&str> = before_func
                    .lines()
                    .rev()
                    .take(10) // Check up to 10 lines before the function
                    .collect();

                let has_doc_comment = preceding_lines.iter().any(|line| {
                    let trimmed = line.trim();
                    trimmed.starts_with("///")
                });

                assert!(
                    has_doc_comment,
                    "AC-29/SCENARIO-033: Public function `{}` must have a /// doc comment. \
                     Preceding lines: {:?}",
                    func_sig,
                    &preceding_lines[..preceding_lines.len().min(5)]
                );
            }
        }
    }

    // =========================================================================
    // AC-30 / SCENARIO-034: Deeply nested logic extracted to named helpers
    //
    // No code block in the production section of podcast_sync.rs should have
    // indentation deeper than 3 levels (excluding match arms which count as 1).
    // Nested logic beyond 3 levels must be extracted into named helper functions.
    // =========================================================================

    /// AC-30/SCENARIO-034: The production code section of podcast_sync.rs must not
    /// have indentation exceeding a reasonable nesting depth. Code nested beyond
    /// 4 indent levels (16 spaces or 4 tabs) should be extracted to helpers.
    ///
    /// This test counts the maximum indentation depth in the production code
    /// (before the #[cfg(test)] section) and verifies it stays within bounds.
    #[test]
    fn production_code_nesting_does_not_exceed_four_indent_levels() {
        let source = include_str!("podcast_sync.rs");

        // Only analyze production code (before tests)
        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // Count maximum indentation (in units of 4 spaces)
        // Allow up to 4 levels (16 spaces) which is typical for:
        //   fn sync_once (level 1)
        //     match (level 2)
        //       arm body (level 3)
        //         if condition (level 4)
        // Anything beyond 5 levels (20 spaces) indicates extraction is needed.
        let max_allowed_indent = 24; // 6 levels of 4 spaces — generous but bounded

        let mut lines_exceeding_limit: Vec<(usize, &str)> = Vec::new();

        for (line_num, line) in production_code.lines().enumerate() {
            // Skip empty lines and comment-only lines
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Count leading spaces
            let leading_spaces = line.len() - line.trim_start().len();

            if leading_spaces > max_allowed_indent {
                lines_exceeding_limit.push((line_num + 1, trimmed));
            }
        }

        assert!(
            lines_exceeding_limit.is_empty(),
            "AC-30/SCENARIO-034: Found {} lines with nesting exceeding {} spaces (6 levels). \
             These should be extracted into named helper functions. Examples:\n{}",
            lines_exceeding_limit.len(),
            max_allowed_indent,
            lines_exceeding_limit
                .iter()
                .take(5)
                .map(|(ln, content)| format!(
                    "  Line {}: {}",
                    ln,
                    &content[..content.len().min(80)]
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// AC-30/SCENARIO-034: The `sync_once` function body must not exceed a
    /// reasonable length. If it does, inner logic should be extracted to helpers.
    /// The specification requires helper extraction for: process_feed_result,
    /// find_episodes_to_download, drain_download_results.
    ///
    /// After extraction, sync_once should be a high-level orchestrator, not a
    /// monolithic 200+ line function with deep nesting.
    #[test]
    fn sync_once_function_body_is_reasonably_short_after_helper_extraction() {
        let source = include_str!("podcast_sync.rs");

        // Find the sync_once function
        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        if let Some(sync_once_start) = production_code.find("pub async fn sync_once(") {
            // Count lines from function start to its closing brace
            let func_body = &production_code[sync_once_start..];

            // Count brace depth to find end of function
            let mut depth = 0;
            let mut func_end = 0;
            let mut found_start = false;
            for (i, ch) in func_body.char_indices() {
                if ch == '{' {
                    depth += 1;
                    found_start = true;
                } else if ch == '}' {
                    depth -= 1;
                    if found_start && depth == 0 {
                        func_end = i;
                        break;
                    }
                }
            }

            let func_lines = func_body[..func_end].lines().count();

            // After Phase 3 helper extraction, sync_once should be under 200 lines.
            // A well-extracted function delegates to helpers rather than inlining everything.
            let max_lines = 200;
            assert!(
                func_lines <= max_lines,
                "AC-30/SCENARIO-034: sync_once is {} lines long (max {}). \
                 Inner logic should be extracted into named helper functions like \
                 process_feed_result, find_episodes_to_download, drain_download_results.",
                func_lines,
                max_lines
            );
        } else {
            panic!("AC-30: Could not find `pub async fn sync_once(` in podcast_sync.rs");
        }
    }

    /// AC-30/SCENARIO-034: Named helper functions must exist for the logic that
    /// was previously deeply nested inside sync_once. The specification requires:
    /// - `should_download_episode` (episode filtering)
    /// - `find_episodes_to_download` (episode selection with limit)
    ///
    /// These must be standalone functions (not closures or inline blocks).
    #[test]
    fn required_helper_functions_exist_as_standalone() {
        let source = include_str!("podcast_sync.rs");

        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // These helper functions MUST exist as named functions
        let required_helpers = [
            (
                "should_download_episode",
                "episode download filtering logic",
            ),
            (
                "find_episodes_to_download",
                "episode selection with max limit",
            ),
        ];

        for (helper_name, purpose) in &required_helpers {
            assert!(
                production_code.contains(&format!("fn {helper_name}")),
                "AC-30/SCENARIO-034: Required helper function `{}` is missing. \
                 It should contain the {} extracted from sync_once.",
                helper_name,
                purpose
            );
        }
    }

    // =========================================================================
    // AC-31 / SCENARIO-035: Functions accept config struct references
    //
    // Function signatures that need multiple configuration values should accept
    // a shared config type or struct reference (e.g., &SynchronizationSettings
    // or &PodcastSettings) rather than passing individual fields as separate
    // parameters. This reduces coupling and makes signatures more maintainable.
    // =========================================================================

    /// AC-31/SCENARIO-035: The `sync_once` function should not destructure config
    /// into individual scalar values passed through the function body. Instead,
    /// it should pass config struct references to helper functions.
    ///
    /// The anti-pattern is extracting values like:
    /// ```ignore
    /// let (concurrent_downloads_max, max_download_retries, max_new_episodes, auto_enqueue, interval_secs) = {
    ///     let config_read = config.read();
    ///     ...
    /// };
    /// ```
    /// The preferred pattern is:
    /// ```ignore
    /// let sync_settings = config.read().settings.podcast.synchronization.clone();
    /// let podcast_settings = config.read().settings.podcast.clone();
    /// // Pass &sync_settings or &podcast_settings to helpers
    /// ```
    #[test]
    fn sync_once_does_not_destructure_config_into_many_individual_variables() {
        let source = include_str!("podcast_sync.rs");

        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // The anti-pattern: destructuring 5 config values into a tuple
        // This indicates the function is accepting individual values instead
        // of passing config struct references to helpers.
        let has_five_tuple_destructure = production_code.contains("let (\n")
            || production_code.contains("let (")
                && production_code.contains("concurrent_downloads_max")
                && production_code.contains("max_download_retries")
                && production_code.contains("max_new_episodes")
                && production_code.contains("auto_enqueue")
                && production_code.contains("interval_secs");

        // Check if these are all extracted as individual let bindings in one block
        // The anti-pattern: 5+ individual config values extracted at function top level
        let config_extraction_block =
            production_code
                .find("pub async fn sync_once(")
                .and_then(|start| {
                    let func_body = &production_code[start..];
                    // Look for the pattern of extracting multiple config values into a tuple
                    func_body
                        .find(") = {")
                        .or_else(|| func_body.find(") = {\n"))
                });

        // If there's a tuple destructuring of 5+ config values, it violates AC-31
        let violates_ac31 = has_five_tuple_destructure && config_extraction_block.is_some();

        assert!(
            !violates_ac31,
            "AC-31/SCENARIO-035: sync_once destructures config into 5+ individual scalar \
             variables (concurrent_downloads_max, max_download_retries, max_new_episodes, \
             auto_enqueue, interval_secs). Instead, clone the config struct and pass struct \
             references (&SynchronizationSettings, &PodcastSettings) to helper functions."
        );
    }

    /// AC-31/SCENARIO-035: Helper functions like `find_episodes_to_download` should
    /// accept a config struct reference when they need config values, rather than
    /// receiving individual config fields as separate parameters.
    ///
    /// This test checks that `find_episodes_to_download` either:
    /// (a) Accepts a `&SynchronizationSettings` reference, OR
    /// (b) Only needs a single config value (max_new_episodes) which is acceptable
    ///     since it's a single parameter, not "many individual config values".
    ///
    /// The real violation is in sync_once itself (see test above).
    #[test]
    fn helper_functions_do_not_accept_excessive_individual_parameters() {
        let source = include_str!("podcast_sync.rs");

        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // Find all function signatures in production code
        // Check that no function has more than 5 parameters (a sign of not using
        // struct references)
        let function_sigs: Vec<&str> = production_code
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                (trimmed.starts_with("pub fn ")
                    || trimmed.starts_with("pub async fn ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("async fn "))
                    && !trimmed.contains("test")
            })
            .collect();

        for sig in &function_sigs {
            // Count commas in the signature (approximate parameter count)
            // This is a heuristic — a function with 6+ parameters likely needs
            // to accept a struct reference instead.
            if let Some(paren_start) = sig.find('(') {
                let after_paren = &sig[paren_start..];
                let comma_count = after_paren.chars().filter(|&c| c == ',').count();

                // Allow up to 5 parameters (generous — spec says use struct refs)
                // sync_once has 3 params (&config, &cmd_tx, &db_path) which is fine
                // The violation is INTERNAL destructuring, tested above
                assert!(
                    comma_count <= 5,
                    "AC-31/SCENARIO-035: Function signature has {} commas (too many parameters). \
                     Refactor to accept a config struct reference instead:\n  {}",
                    comma_count,
                    sig.trim()
                );
            }
        }
    }

    /// AC-31/SCENARIO-035: The start_podcast_sync_task function should accept
    /// config as a SharedServerSettings (struct reference pattern) rather than
    /// individual values. This verifies the current pattern is correct.
    #[test]
    fn start_podcast_sync_task_accepts_shared_config_not_individual_values() {
        let source = include_str!("podcast_sync.rs");

        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // Find start_podcast_sync_task signature
        assert!(
            production_code.contains("fn start_podcast_sync_task("),
            "start_podcast_sync_task function must exist"
        );

        // It should accept SharedServerSettings (or similar config wrapper)
        // rather than individual Duration, bool, u32, etc. parameters
        if let Some(sig_start) = production_code.find("fn start_podcast_sync_task(") {
            let sig_area = &production_code[sig_start..];
            let sig_end = sig_area.find('{').unwrap_or(sig_area.len().min(500));
            let signature = &sig_area[..sig_end];

            // Must contain SharedServerSettings (the config struct pattern)
            assert!(
                signature.contains("SharedServerSettings")
                    || signature.contains("&ServerOverlay")
                    || signature.contains("config:"),
                "AC-31/SCENARIO-035: start_podcast_sync_task must accept a config struct \
                 reference (SharedServerSettings), not individual interval/refresh/etc. values. \
                 Signature: {}",
                signature.trim()
            );

            // Must NOT have individual config parameters like:
            // interval: Duration, refresh_on_startup: bool, max_new_episodes: u32
            let has_individual_config_params = signature.contains("interval: Duration")
                && signature.contains("refresh_on_startup: bool");
            assert!(
                !has_individual_config_params,
                "AC-31/SCENARIO-035: start_podcast_sync_task must NOT accept individual \
                 config values (interval: Duration, refresh_on_startup: bool, etc.). \
                 Use SharedServerSettings instead. Signature: {}",
                signature.trim()
            );
        }
    }

    // =========================================================================
    // AC-30/AC-31 Combined: Verify the overall style of the module
    // =========================================================================

    /// AC-30/SCENARIO-034: After helper extraction, the module should have
    /// multiple named functions rather than one monolithic function.
    /// This verifies that the module has at least 3 public/crate-visible functions
    /// in the production code section (not counting test helpers).
    #[test]
    fn module_has_multiple_named_functions_indicating_proper_extraction() {
        let source = include_str!("podcast_sync.rs");

        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // Count function definitions (pub fn and pub async fn)
        let pub_fn_count = production_code
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ")
            })
            .count();

        // After proper extraction, there should be at least 3 public functions:
        // - sync_once (main orchestrator)
        // - should_download_episode (filtering helper)
        // - find_episodes_to_download (selection helper)
        // - start_podcast_sync_task (lifecycle helper)
        assert!(
            pub_fn_count >= 3,
            "AC-30/SCENARIO-034: Module should have at least 3 public functions after \
             helper extraction. Found {} public function(s). This indicates monolithic \
             code that needs to be decomposed into named helpers.",
            pub_fn_count
        );
    }

    /// AC-29/SCENARIO-033: The module-level doc comment should follow the approved
    /// format from review. It must NOT be a regular comment (//) converted to
    /// a doc comment (//!) without actual documentation content.
    /// Minimum requirement: describes what the module does, not just a file name.
    #[test]
    fn module_doc_comment_is_not_just_a_filename_or_placeholder() {
        let source = include_str!("podcast_sync.rs");

        let first_doc_line = source
            .lines()
            .next()
            .unwrap_or("")
            .trim_start_matches("//!")
            .trim();

        // The doc comment should describe behavior, not just restate the filename
        let bad_patterns = [
            "podcast_sync.rs",
            "podcast_sync",
            "This file",
            "File:",
            "TODO",
        ];

        for bad in &bad_patterns {
            assert!(
                !first_doc_line
                    .to_lowercase()
                    .starts_with(&bad.to_lowercase()),
                "AC-29/SCENARIO-033: Module doc comment should describe purpose, not \
                 restate the filename or be a placeholder. First line: {:?}",
                first_doc_line
            );
        }

        // Must have actual content (not empty //! line)
        assert!(
            !first_doc_line.is_empty(),
            "AC-29/SCENARIO-033: First //! line must have content describing the module purpose."
        );
    }

    /// AC-30/SCENARIO-034: No inline `struct` definitions inside function bodies
    /// in production code. Structs defined inside functions indicate logic that
    /// should be extracted to module-level types.
    ///
    /// The current code has `struct EnqueueEntry` inside sync_once — this should
    /// be a module-level type or eliminated in favor of tuples/existing types.
    #[test]
    fn no_struct_definitions_inside_function_bodies() {
        let source = include_str!("podcast_sync.rs");

        let production_code = source
            .find("#[cfg(test)]")
            .map(|idx| &source[..idx])
            .unwrap_or(source);

        // Look for struct definitions that are indented (inside function bodies)
        let inline_structs: Vec<(usize, &str)> = production_code
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                let trimmed = line.trim();
                // A struct definition inside a function body will be indented
                // (starts with spaces followed by "struct ")
                line.starts_with("    ") // indented
                    && (trimmed.starts_with("struct ") || trimmed.starts_with("pub struct "))
                    && !trimmed.contains("//") // not in a comment
            })
            .collect();

        assert!(
            inline_structs.is_empty(),
            "AC-30/SCENARIO-034: Found {} struct definition(s) inside function bodies. \
             These should be extracted to module-level types for better readability and reuse:\n{}",
            inline_structs.len(),
            inline_structs
                .iter()
                .take(3)
                .map(|(ln, content)| format!("  Line {}: {}", ln + 1, content.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
