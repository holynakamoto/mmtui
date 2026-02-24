class Mmtui < Formula
  desc "Terminal user interface for NCAA March Madness brackets"
  homepage "https://github.com/holynakamoto/mmtui"
  url "https://github.com/holynakamoto/mmtui/archive/refs/tags/v0.1.7.tar.gz"
  sha256 "1747bbbfcf8a3f7f745596e2679d5a0e384d03043adeb751c6faae6c6e2d8e8e"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mmtui --version")
  end
end
