import { Tokenizer, type MarkedExtension } from 'marked'

// CommonMark does not consider `。**下` right-flanking: punctuation before
// the delimiter and a letter after it makes it look like an opener. Chinese
// prose routinely omits the space between sentences (`**重要。**下一句`).
const CJK_EMPHASIS_CLOSE_RE = /[。．.!！？?!](\*+)(?=[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff])/g

/** Relax sentence-ending emphasis boundaries without rewriting Markdown. */
export function createCjkEmphasisExtension(): MarkedExtension {
  return {
    tokenizer: {
      emStrong(source, maskedSource, previousCharacter) {
        // Underscores retain CommonMark's intraword restrictions.
        if (source[0] !== '*') return false

        // Only change marked's delimiter-scanning mask, never source or tokens.
        // A same-width word character makes this boundary right-flanking while
        // retaining all source offsets. Marked already masks code, links and
        // escapes here; its own algorithm still owns nesting and delimiter runs.
        const mask = maskedSource.replace(CJK_EMPHASIS_CLOSE_RE, 'a$1')
        return Tokenizer.prototype.emStrong.call(this, source, mask, previousCharacter)
      },
    },
  }
}
