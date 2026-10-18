Pod::Spec.new do |s|
  s.name = 'BopsieIcons'
  s.version = '1.0.0'
  s.summary = 'Synchronous bundled Feather vector icons for Bopsie'
  s.description = s.summary
  s.license = { :type => 'MIT', :file => '../LICENSE-Feather' }
  s.author = 'Bopsie'
  s.homepage = 'https://github.com/Pajn/bopsie'
  s.source = { :git => 'https://github.com/Pajn/bopsie.git' }
  s.platforms = { :ios => '16.4' }
  s.swift_version = '5.9'
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.source_files = '**/*.swift'
  s.resource_bundles = { 'BopsieIcons' => ['Icons.xcassets'] }
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
