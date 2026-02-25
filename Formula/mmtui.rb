class Mmtui < Formula
  desc "Terminal user interface for NCAA March Madness brackets"
  homepage "https://github.com/holynakamoto/mmtui"
  url "https://github.com/holynakamoto/mmtui/archive/refs/tags/v0.1.9.tar.gz"
  sha256 "0a49727504bafe244f148bd6c1591c91f3c80afc007289c022dfcdb7167ba000"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mmtui --version")
  end
end
