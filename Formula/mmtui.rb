class Mmtui < Formula
  desc "Terminal user interface for NCAA March Madness brackets"
  homepage "https://github.com/holynakamoto/mmtui"
  license "MIT"

  # Head formula so users can install directly from the tap without waiting
  # for release artifact wiring.
  head "https://github.com/holynakamoto/mmtui.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match "mmtui", shell_output("#{bin}/mmtui --version")
  end
end
