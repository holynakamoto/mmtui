class Mmtui < Formula
  desc "Terminal user interface for NCAA March Madness brackets"
  homepage "https://github.com/holynakamoto/mmtui"
  url "https://github.com/holynakamoto/mmtui/archive/refs/tags/v0.1.5.tar.gz"
  sha256 "373c9991bdfec1a13966d7b25a883e266d6e48227ebd467411afaf8cc4e540a6"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mmtui --version")
  end
end
